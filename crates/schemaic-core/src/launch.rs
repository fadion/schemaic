//! What may be handed to an OS launcher — the boundary where a string the app
//! merely *displayed* becomes a string the operating system *executes*.
//!
//! Every function here answers one question: is this byte sequence safe to put
//! in front of a process launcher, and if so, in what shape. The shape is part
//! of the answer, which is why [`url_open_argv`] returns the whole argv rather
//! than a bare `bool` — a URL is only as safe as the program it is handed to,
//! and a review found a clicked terminal link running an arbitrary command
//! purely because the launcher in between was `cmd /C`.
//!
//! Two rules hold for everything in this module:
//!
//! 1. **No shell — with one named exception, and it is named here rather than
//!    assumed away.** The argv this module produces is executed directly:
//!    nothing it emits is handed to `cmd`, `sh` or PowerShell *by this app*, so
//!    a byte that is syntax to a shell is inert on the way out.
//!    [`url_open_argv`]'s own test pins that, because the alternative —
//!    filtering every metacharacter of every shell — is how `&` would come to
//!    be refused inside a query string while `%` still expanded.
//!
//!    The exception is **`xdg-open`**, the Linux arm of [`url_open_argv`]. It
//!    is not a binary: `xdg-utils` ships it as a `#!/bin/sh` script, so on
//!    Linux the one program this module names is itself an interpreter. The
//!    Windows and macOS arms (`explorer`, `open`) are genuine executables and
//!    the rule holds there exactly as stated.
//!
//!    What that costs, stated rather than left to be discovered: the bytes
//!    [`is_url_byte`] admits — `& ; $ ' ( ) *` — reach that script's own
//!    parsing, and the argument that they are inert there is an argument about
//!    **somebody else's code**, which nothing in this repository re-checks when
//!    `xdg-utils` changes. It was not verified in writing this note (no
//!    `xdg-utils` on the machine at hand), and saying so is the point: the URL
//!    still travels as *one* argv element, still passes RFC 3986's allowlist,
//!    and still carries no byte outside it — which is everything this module
//!    can guarantee on its own. Removing the dependence means preferring a
//!    resolvable `$BROWSER`/`gio open` on the Linux arm; until then this
//!    paragraph is the guarantee.
//! 2. **The string is validated where it stops being data**, not at the site
//!    that produced it. A caller upstream may have its own reasons to be
//!    permissive (the terminal's link tagger admits `&` because a query string
//!    needs it); the launcher does not inherit those reasons.

/// Why a client or tool cannot be handed what it was about to be — every
/// refusal this module returns, and [`AuthMode::external_client_refusal`]'s.
///
/// **A value, not a sentence**, so the terminal panel can word it in the
/// interface's language; [`Refusal::message`] is the English, which the
/// interface's renderer is held equal to. **No `Display`, on purpose**: no
/// headless surface says one, and with it a caller's `why.to_string()` would
/// compile and stay English in every language.
///
/// [`AuthMode::external_client_refusal`]: crate::connection::AuthMode::external_client_refusal
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The client resolved is a `.cmd`/`.bat` shim ([`direct_spawn_verdict`]).
    ClientIsShim,
    /// The database name holds an `=` `psql` would read as conninfo
    /// ([`psql_target`]).
    NameHasEquals,
    /// The database name begins like a connection URI ([`psql_target`]).
    NameIsUri,
    /// The SQLite file's name begins with `-` ([`sqlite_target`]).
    FileStartsWithDash,
    /// `sqlcmd` cannot be handed an Entra token ([`sqlcmd_args`]).
    SqlcmdEntra,
    /// The host holds address syntax `sqlcmd` would read ([`sqlcmd_args`]).
    SqlcmdHost,
    /// `sqlcmd` cannot verify against a CA file ([`sqlcmd_args`]).
    SqlcmdCaFile,
    /// SQL Server signs no one in by a client certificate ([`sqlcmd_args`]).
    SqlcmdClientCert,
    /// A certificate path is a Windows path the WSL client cannot open
    /// ([`wsl_tls_blocker`]).
    WslCertificatePaths,
    /// A verifying rung through an SSH tunnel ([`tunnelled_verify_blocker`]).
    TunnelledVerify,
    /// `docker ps` printed something other than an id
    /// ([`docker_inspect_args`]).
    NotAContainerId,
    /// The backup or restore file's name holds a line break
    /// ([`backup_file_target`]).
    FileNameLineBreak,
    /// The init SQL's role cannot be passed as `--role` (`role_arg`).
    RoleUnpassable,
    /// The `mysql` client would read the path as commands
    /// ([`mysql_source_path`]).
    MysqlSourcePath,
    /// The connection signs in by a cloud CLI's token, which an external
    /// client is not handed.
    CloudToken,
}

impl Refusal {
    /// The refusal in English — the wording the interface's `en-GB`
    /// catalogue is held to.
    pub fn message(self) -> &'static str {
        match self {
            Refusal::ClientIsShim => {
                "The client found on your PATH is a .cmd/.bat shim, which Windows runs \
                 through cmd.exe — where a database name carrying '&' or '|' would be a second \
                 command. Point Settings at the real executable, or open a query tab instead."
            }
            Refusal::NameHasEquals => {
                "This database's name contains '=', which psql would read as a connection \
                 string and follow to another server. Open it in a query tab instead."
            }
            Refusal::NameIsUri => {
                "This database's name begins like a connection URI, which psql would follow to \
                 another server. Open it in a query tab instead."
            }
            Refusal::FileStartsWithDash => {
                "This database file's name begins with '-', which the sqlite3 client would read \
                 as an option rather than as the file. Rename it, or give it a path (./name.db), \
                 and open it again."
            }
            Refusal::SqlcmdEntra => {
                "sqlcmd cannot be handed the Azure CLI's sign-in token, so this connection \
                 cannot open in it. Open it in a query tab instead."
            }
            Refusal::SqlcmdHost => {
                "This connection's host contains a character sqlcmd reads as part of the \
                 address (',', ';', a second '\\' or a ':' prefix). Set the port in its own \
                 field, and open it again."
            }
            Refusal::SqlcmdCaFile => {
                "sqlcmd cannot verify against a CA file. Add the CA to the system's trust store \
                 and clear the connection's CA field, or open the connection in a query tab."
            }
            Refusal::SqlcmdClientCert => {
                "SQL Server does not sign in with a client certificate, so sqlcmd has nowhere \
                 to send this connection's. Clear the client certificate and key, and open it \
                 again."
            }
            Refusal::WslCertificatePaths => {
                "This connection's TLS certificate paths are Windows paths, which the WSL \
                 client cannot open. Install a native client, or move the certificates inside \
                 WSL."
            }
            Refusal::TunnelledVerify => {
                "This connection verifies the server's certificate and reaches it through an \
                 SSH tunnel, so the client would be told to dial 127.0.0.1 and check the \
                 certificate against that — which the server's own certificate cannot satisfy. \
                 Open a query tab instead, or lower the TLS mode for the CLI session."
            }
            Refusal::NotAContainerId => "docker ps printed something that is not a container id.",
            Refusal::FileNameLineBreak => {
                "That file name contains a line break, which the client would read as two \
                 arguments. Choose another name."
            }
            Refusal::RoleUnpassable => {
                "The role in this connection's init SQL cannot be handed to the tool as it is."
            }
            Refusal::MysqlSourcePath => {
                "The mysql client would read that file's path as commands of its own (a ';' or \
                 a '\\' in it, or a space at an end). Move or rename it, then restore it."
            }
            Refusal::CloudToken => {
                "This connection signs in through a cloud CLI's token, which an external client \
                 is not handed. Open it in a query tab instead."
            }
        }
    }

    /// Every refusal, for the tests that hold a rendering to [`Self::message`].
    pub const ALL: [Refusal; 15] = [
        Refusal::ClientIsShim,
        Refusal::NameHasEquals,
        Refusal::NameIsUri,
        Refusal::FileStartsWithDash,
        Refusal::SqlcmdEntra,
        Refusal::SqlcmdHost,
        Refusal::SqlcmdCaFile,
        Refusal::SqlcmdClientCert,
        Refusal::WslCertificatePaths,
        Refusal::TunnelledVerify,
        Refusal::NotAContainerId,
        Refusal::FileNameLineBreak,
        Refusal::RoleUnpassable,
        Refusal::MysqlSourcePath,
        Refusal::CloudToken,
    ];
}

/// The bytes a URL may contain, as an allowlist.
///
/// The unreserved and reserved sets of RFC 3986 — nothing else. That excludes
/// every ASCII control byte, the space, and the whole of `" < > ^ | \` `` ` ``
/// `{ }`, which is the intersection worth naming: those are bytes `cmd` and
/// `sh` treat as syntax and RFC 3986 has no use for. The sub-delims it *does*
/// admit — `&`, `;`, `$`, `'`, `(`, `)` — are syntax to one shell or another
/// and are kept anyway, because a real query string needs them and
/// [`url_open_argv`] hands them to no shell **of this app's making** — see
/// rule 1 in the module header for the one interpreter that is nonetheless in
/// the set on Linux, and for what is and is not guaranteed about it.
/// Non-ASCII is excluded too, and
/// deliberately: the terminal's link tagger builds its candidates from
/// `is_ascii_alphanumeric`, so a non-ASCII byte cannot have come from the one
/// caller this gate has.
fn is_url_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b"-._~:/?#[]@!$&'()*+,;=%".contains(&b)
}

/// Does `s` end with `suffix`, comparing ASCII case-insensitively?
///
/// **The one spelling of "is this the file extension", and it exists because
/// the obvious spelling panics.** Both call sites wrote
/// `s.len() > e.len() && s[s.len() - e.len()..].eq_ignore_ascii_case(e)`, which
/// guards the index against being *out of range* and not against being inside a
/// character — and indexing a `&str` requires a char boundary. `PATHEXT`'s
/// entries are 3 and 4 bytes, so any name whose last 3 or 4 bytes straddle a
/// multi-byte character panicked: `naïve` typed into Settings → AI → CLI path
/// took the app down per keystroke, because the field is validated on every
/// value rather than on commit. `克劳德` and `日本` do it too; `café` does not,
/// which is why eight ASCII tests and a hand check never saw it.
///
/// Lives here rather than beside either caller because the two are in different
/// crates and this is the same question in both — which extension a program name
/// carries, at the boundary where a string becomes a process.
pub fn ends_with_ignore_ascii_case(s: &str, suffix: &str) -> bool {
    s.len() > suffix.len()
        && s.as_bytes()[s.len() - suffix.len()..].eq_ignore_ascii_case(suffix.as_bytes())
}

/// May this **resolved** program be spawned directly, or is there a shell in
/// between? `Err` carries the line the panel shows instead of a session.
///
/// **The one precondition this module's rule 1 does not survive.** On Windows,
/// `CreateProcess` on a `.cmd`/`.bat` image runs it through `cmd.exe` — the
/// CVE-2024-24576 class — and the library this path spawns through
/// (`portable-pty`) quotes an argument only when it holds a space, tab,
/// newline, VT or `"`. So `&`, `|`, `^`, `%VAR%` and `<`/`>` pass through raw
/// into a command line `cmd` then parses. Measured on Windows 11 with a two-line
/// `shim.cmd`: a database named `db&echo PWNED` ran the second command.
///
/// That defeats the `--` terminator the DB-CLI argv puts in front of a
/// server-supplied database name: `--` is *my_getopt*'s option terminator and
/// means nothing to `cmd`. And `--` is the whole reason `mysql_shell_config` may
/// pass such a name through at all — a name written by anyone who may
/// `CREATE DATABASE` on a shared server.
///
/// A `.cmd` client on `PATH` is a supported configuration rather than a
/// misconfiguration: `shell::which`'s own `PATHEXT` default spells out
/// `.EXE;.CMD;.BAT;.COM`, and wrapper shims are how npm- and pipx-style
/// launchers ship. So this refuses rather than pretending, the way
/// [`psql_target`] does for a conninfo-shaped name.
///
/// Only the DB-CLI launchers ask. The user's own terminal shell is a shell by
/// definition and is not this question.
pub fn direct_spawn_verdict(resolved: &std::path::Path) -> Result<(), Refusal> {
    let is_shim = resolved
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("cmd") || e.eq_ignore_ascii_case("bat"));
    if is_shim {
        Err(Refusal::ClientIsShim)
    } else {
        Ok(())
    }
}

/// A URL the app may hand to the OS default browser, or `None`.
///
/// Requires an `http`/`https` scheme (ASCII-case-insensitively — the scheme is
/// case-insensitive per RFC 3986 and the guard this replaced was not), a
/// non-empty authority after it, [`is_url_byte`] for every byte, and
/// well-formed percent-encoding. That last one is not pedantry: `%windir%` is a
/// legal-looking path fragment and `cmd` expands it before it parses anything,
/// so a `%` that is not the start of a `%XX` escape is the tell of a string
/// written for a shell rather than for a browser.
///
/// `&` is **not** refused. It is a query separator, half the real links a
/// terminal prints carry one, and it is dangerous only when a shell reads it —
/// which [`url_open_argv`] guarantees none does.
pub fn openable_url(raw: &str) -> Option<&str> {
    let rest = strip_scheme(raw)?;
    if rest.is_empty() {
        return None;
    }
    let bytes = raw.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if !is_url_byte(b) {
            return None;
        }
        if b == b'%'
            && !bytes
                .get(i + 1..i + 3)
                .is_some_and(|pair| pair.iter().all(u8::is_ascii_hexdigit))
        {
            return None;
        }
    }
    Some(raw)
}

/// The part of `raw` after an `http://`/`https://` scheme, or `None`.
fn strip_scheme(raw: &str) -> Option<&str> {
    for scheme in ["https://", "http://"] {
        // `get`, not `[..n]`: a length guard proves the slice is in range and
        // says nothing about `n` being a char boundary, which indexing a `&str`
        // requires. `https:/é` is 9 bytes, so the guard passed and byte 8 was
        // inside the `é` — a panic, on the thread that draws the window.
        if raw
            .get(..scheme.len())
            .is_some_and(|p| p.eq_ignore_ascii_case(scheme))
        {
            return Some(&raw[scheme.len()..]);
        }
    }
    None
}

/// The argv that opens `raw` in the OS default browser: element 0 is the
/// program, the rest are its arguments. `None` when [`openable_url`] refuses.
///
/// The program is never a shell on any platform — see the module rules. On
/// Windows that means `explorer`, which takes argv and is already this app's
/// launcher for a folder; the `cmd /C start "" <url>` idiom it replaces existed
/// only to get a detached browser, which `explorer` gives directly.
pub fn url_open_argv(raw: &str) -> Option<Vec<String>> {
    let url = openable_url(raw)?;
    let program = if cfg!(windows) {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    Some(vec![program.to_string(), url.to_string()])
}

/// The database name `psql` may be given as the value of `-d`, or the reason it
/// may not.
///
/// **libpq re-parses that value.** A `-d` argument containing an `=` sign, or
/// beginning with a `postgresql://`/`postgres://` prefix, is taken as a whole
/// *conninfo string* — so `-h <the real host>` three arguments earlier is
/// silently overridden by whatever `host=` the value carries, and the session,
/// with `PGPASSWORD` in its environment, goes to a server of the value's
/// choosing. Measured against PostgreSQL 16: `CREATE DATABASE "dbname=postgres
/// host=192.0.2.1"` succeeds, appears in the schema tree like any other
/// database, and "Open in CLI" on it dials `192.0.2.1`.
///
/// The name comes from the server, so it is not the user's to be trusted with:
/// on a shared or compromised server anyone who may `CREATE DATABASE` writes
/// this string. psql has no `--` terminator to hide behind either — the
/// detection is on the *value*, not the position — so the answer is a refusal
/// with a message, which is a shape the caller already has for "no client
/// found".
pub fn psql_target(db: &str) -> Result<&str, Refusal> {
    const URI_PREFIXES: [&str; 2] = ["postgresql://", "postgres://"];
    if db.contains('=') {
        return Err(Refusal::NameHasEquals);
    }
    // `get`, not `[..n]` — see `strip_scheme`. The name is read verbatim out of
    // `pg_database.datname`, is not the user's to vouch for, and this runs on
    // the UI thread from a click handler, where a panic takes the app rather
    // than reaching the panel the way every other refusal here does.
    if URI_PREFIXES
        .iter()
        .any(|p| db.get(..p.len()).is_some_and(|s| s.eq_ignore_ascii_case(p)))
    {
        return Err(Refusal::NameIsUri);
    }
    Ok(db)
}

/// Which file `sqlite3` should open.
///
/// [`psql_target`]'s sibling, and the third member of the family: the MySQL
/// builder pushes `--` before the database name, the PostgreSQL one comes
/// through `psql_target`, and the SQLite one handed `conn.file` to argv with
/// neither. `sqlite3` reads a leading `-` as an option, so a file named
/// `-tmp.db` — typed, or carried in from a DBeaver/HeidiSQL import — either
/// errors incomprehensibly or opens a session against something other than the
/// file the schema tree is showing. The second outcome is the one
/// `sqlite_shell`'s own doc gives as the reason its WSL path is refused:
/// "hands the user a session on something that looks like theirs".
///
/// A leading `-` is the whole of the question for this client — there is no
/// connection-string syntax to be re-read into, which is what makes
/// [`psql_target`] the longer of the two. The refusal is a `Result` rather than
/// a caller's `if`, so it lands on the one arm `open_db_cli` already had.
pub fn sqlite_target(file: &str) -> Result<&str, Refusal> {
    if file.starts_with('-') {
        return Err(Refusal::FileStartsWithDash);
    }
    Ok(file)
}

/// A connection's own setting — a host, a user, a region — as the **value** of
/// a cloud CLI's flag (`cloud_iam`'s `aws rds generate-db-auth-token
/// --hostname <host>`).
///
/// The CLIs parse their argv as options wherever a word begins with `-`, so a
/// user named `--profile=prod` would sign in as somebody else's profile rather
/// than fail; and a control character has no business in any of the three.
/// Empty is refused too: a flag with no value takes the next flag as one.
pub fn cli_flag_value<'a>(what: &str, value: &'a str) -> Result<&'a str, String> {
    if value.is_empty() {
        return Err(format!("The connection has no {what} to sign in with."));
    }
    if value.starts_with('-') {
        return Err(format!(
            "The connection's {what} begins with '-', which the cloud CLI would read as an \
             option rather than as the {what}."
        ));
    }
    if value.chars().any(char::is_control) {
        return Err(format!(
            "The connection's {what} holds a control character."
        ));
    }
    Ok(value)
}

/// Which of the two MySQL-family clients an argv is being built for.
///
/// **They do not share the option.** `--ssl-mode` is MySQL's; MariaDB's client
/// uses `my_getopt` and has no such variable, so the flag is not merely
/// unidiomatic there — the client exits immediately with
/// `unknown option '--ssl-mode=DISABLED'` and the terminal panel shows that
/// instead of a session. At *every* rung, the default included, because the flag
/// was emitted unconditionally. MariaDB is this project's own primary engine
/// and its own live tier, so this was the feature broken outright for the
/// commonest install.
///
/// Resolved from the program name rather than guessed: `resolve_cli` accepts
/// either binary, and the WSL fallback always spells it `mysql`, which on a
/// modern MariaDB install is a symlink to the MariaDB client — so a name of
/// `mysql` cannot mean "MySQL's client" and the only honest reading is the one
/// the user's own `PATH` gave.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MysqlClient {
    /// Oracle MySQL's `mysql`.
    Mysql,
    /// MariaDB's `mariadb` (and the `mysql` symlink that points at it).
    MariaDb,
}

impl MysqlClient {
    /// Which client the program named `prog` is.
    ///
    /// **`mysql` stays Oracle's, and that is a known residual.** On a modern
    /// MariaDB install `mysql` is a symlink to the MariaDB client, and no
    /// inspection of the *name* can tell the two apart — while the two spellings
    /// are mutually fatal (`--ssl-mode` is unknown to MariaDB's client, and
    /// MySQL removed `--ssl` in 8.0.26). So this closes the case the review
    /// measured — a box whose only client is `mariadb`, which `resolve_cli`
    /// falls through to — and leaves the symlink case as it was rather than
    /// trading it for a new one. Asking the client its version is the real
    /// answer and is not a thing this pure function can do.
    pub fn of(prog: &str) -> Self {
        let stem = prog
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(prog)
            .split('.')
            .next()
            .unwrap_or(prog);
        // MariaDB's dump tool is its client family too, and spells TLS as
        // its client does.
        if stem.eq_ignore_ascii_case("mariadb") || stem.eq_ignore_ascii_case("mariadb-dump") {
            MysqlClient::MariaDb
        } else {
            MysqlClient::Mysql
        }
    }
}

/// The `--ssl-*` argv a MySQL/MariaDB client needs to honour `tls`.
///
/// **Restored to the function it is about.** `44ccfa4` inserted
/// [`MysqlClient`]'s doc between this block and its signature, so this paragraph
/// became the opening of the published rustdoc for *that enum* and this function
/// carried no doc at all — `missing_docs` is off, so `cargo doc -D warnings` saw
/// nothing. The contract below is the one thing a reader of this function has to
/// know, and for a while it was attached to something else.
///
/// **Non-empty wherever saying nothing would change the meaning.** For MySQL's
/// client that is every rung including
/// [`SslMode::Disable`][crate::connection::SslMode::Disable]: its default is
/// `--ssl-mode=PREFERRED`, which accepts an unencrypted socket if the server
/// offers one and verifies no certificate either way — so an omitted flag
/// downgrades a `verify-full` connection while the app's own header, whose
/// socket really is encrypted, goes on reporting TLS.
///
/// MariaDB's client has one rung where the empty answer is the right one, and it
/// is measured rather than assumed — see the `Prefer` arm.
pub fn mysql_cli_tls_args(tls: &crate::connection::Tls, client: MysqlClient) -> Vec<String> {
    use crate::connection::SslMode;
    if client == MysqlClient::MariaDb {
        // MariaDB's spelling of the same five rungs. `--ssl` enables TLS,
        // `--skip-ssl` refuses it, and `--ssl-verify-server-cert` is the one
        // that checks the certificate names the host — the three long-standing
        // `my_getopt` variables its client does have.
        let mut args = match tls.mode {
            SslMode::Disable => vec!["--skip-ssl".to_string()],
            // The client's own default already tries TLS and verifies nothing,
            // which is what this rung means — so there is nothing to say, and
            // saying `--ssl` would turn a preference into a requirement.
            //
            // **Measured, because the sentence above was originally written
            // about *MySQL's* client and inherited here.** MariaDB 10.11.14's
            // client, invoked with no `--ssl` flag at all against a server with
            // `have_ssl = YES`, reports
            // `Ssl_cipher = TLS_AES_256_GCM_SHA384` — it is opportunistic, so
            // the empty answer really is this rung. Had it come back blank, the
            // CLI session would have been plaintext while the app's own driver
            // on the same `Connection` was encrypted and the terminal panel
            // badged it as this connection's client.
            SslMode::Prefer => Vec::new(),
            SslMode::Require | SslMode::VerifyCa => vec!["--ssl".to_string()],
            SslMode::VerifyFull => {
                vec!["--ssl".to_string(), "--ssl-verify-server-cert".to_string()]
            }
        };
        if let Some(ca) = tls.ca_file() {
            args.push(format!("--ssl-ca={ca}"));
        }
        if tls.uses_client_cert() {
            args.push(format!("--ssl-cert={}", tls.client_cert_path));
            args.push(format!("--ssl-key={}", tls.client_key_path));
        }
        return args;
    }
    let mode = match tls.mode {
        SslMode::Disable => "DISABLED",
        SslMode::Prefer => "PREFERRED",
        SslMode::Require => "REQUIRED",
        SslMode::VerifyCa => "VERIFY_CA",
        SslMode::VerifyFull => "VERIFY_IDENTITY",
    };
    let mut args = vec![format!("--ssl-mode={mode}")];
    if let Some(ca) = tls.ca_file() {
        args.push(format!("--ssl-ca={ca}"));
    }
    if tls.uses_client_cert() {
        args.push(format!("--ssl-cert={}", tls.client_cert_path));
        args.push(format!("--ssl-key={}", tls.client_key_path));
    }
    args
}

/// The environment a `psql` session needs to honour `tls`.
///
/// **Environment rather than argv, and not for symmetry with the password.**
/// psql has no `--sslmode` flag at all: the setting exists only inside a
/// conninfo string, which is the one thing [`psql_target`] refuses to let this
/// path build. `PGSSLMODE` and friends are libpq's own supported spelling of
/// the same parameters and are parsed as values, never as syntax.
pub fn psql_cli_tls_env(tls: &crate::connection::Tls) -> Vec<(String, String)> {
    use crate::connection::SslMode;
    let mode = match tls.mode {
        SslMode::Disable => "disable",
        SslMode::Prefer => "prefer",
        SslMode::Require => "require",
        SslMode::VerifyCa => "verify-ca",
        SslMode::VerifyFull => "verify-full",
    };
    let mut env = vec![("PGSSLMODE".to_string(), mode.to_string())];
    if let Some(ca) = tls.ca_file() {
        env.push(("PGSSLROOTCERT".to_string(), ca.to_string()));
    }
    if tls.uses_client_cert() {
        env.push(("PGSSLCERT".to_string(), tls.client_cert_path.clone()));
        env.push(("PGSSLKEY".to_string(), tls.client_key_path.clone()));
    }
    env
}

/// `sqlcmd`'s argv for a SQL Server session — the password travels as
/// `SQLCMDPASSWORD`, never here.
///
/// **Every value is attached to its flag** (`-d<name>`, not `-d <name>`), so
/// none can be read as an option by either `sqlcmd` — the ODBC one or the Go
/// rewrite, which share the name and the flags. Measured against ODBC
/// `sqlcmd` 18: the database is taken verbatim, so a name like
/// `x;Encrypt=no;Server=192.0.2.1` opens *that database* on *this server* —
/// there is no connection-string re-read to refuse, which is the difference
/// from [`psql_target`]. The host is the one value `sqlcmd` parses
/// (`host\instance`, `host,port`, a `tcp:`, `np:`, `lpc:` or `admin:` prefix), so it is forced
/// onto TCP with `tcp:` and one that would re-shape the address is refused —
/// bar a named instance, `server\INSTANCE`
/// ([`crate::connection::sql_server_instance`]), which is passed on as
/// `tcp:server\INSTANCE` with no port, `sqlcmd` asking SQL Server Browser for
/// it as the app's own connect does.
/// `-I` turns `QUOTED_IDENTIFIER` on, as every Schemaic session has it: the
/// client's default is off, where an index on a computed column is Msg 1934.
///
/// The TLS rungs map onto what `sqlcmd` can express, measured the same way:
/// `-No` is no encryption past the login (`disable`); `-Nm -C` encrypts
/// without judging the certificate (`prefer` and `require` — SQL Server
/// always offers TLS, so `prefer` has nothing to fall back from); `-Nm` alone
/// verifies the chain and the name against the system store (`verify-ca`
/// gets the name checked too, the stricter answer). A private CA file is
/// refused: `sqlcmd` has no option for one (`-J` pins the server's own
/// certificate, a different trust), and dropping it would verify against a
/// store the connection did not ask for. So is a client certificate, for the
/// same reason: TDS signs no one in by one, so there is nowhere to send it.
///
/// **It signs in as the connection does** (`auth`, already the connection's
/// `effective_auth`): `-U` for a SQL login, `-E` — a trusted connection, the
/// same flag in both clients — for Windows sign-in, and a refusal for Entra,
/// whose token `sqlcmd` has no way to be handed.
pub fn sqlcmd_args(
    host: &str,
    port: u16,
    auth: crate::connection::AuthMode,
    user: &str,
    database: Option<&str>,
    tls: &crate::connection::Tls,
) -> Result<Vec<String>, Refusal> {
    use crate::connection::{AuthMode, SslMode};
    if tls.uses_client_cert() {
        return Err(Refusal::SqlcmdClientCert);
    }
    // A named instance is `sqlcmd`'s own spelling too — `tcp:server\INSTANCE`,
    // which it resolves through SQL Server Browser as the connect does — so it
    // is passed on as one, with no port: the saved one is not the instance's.
    //
    // Read as the connect reads it (`connection::sql_server_address`): a
    // `tcp:` prefix, `(local)` and an inline `,port` resolved, so the text
    // that connects in a query tab opens here too (S5-L1-02). What is left
    // that sqlcmd would read as address syntax is still refused.
    let address = crate::connection::sql_server_address(host);
    let instance = address.instance;
    let host = address.server;
    let port = address.port.unwrap_or(port);
    let prefixed = ["tcp:", "np:", "lpc:", "admin:"].iter().any(|p| {
        host.get(..p.len())
            .is_some_and(|s| s.eq_ignore_ascii_case(p))
    });
    if host.is_empty()
        || prefixed
        || host.contains([',', '\\', ';'])
        || instance.is_some_and(|i| i.contains([',', ';', ':']))
    {
        return Err(Refusal::SqlcmdHost);
    }
    let host = if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host.to_string()
    };
    let address = match instance {
        Some(instance) => format!("-Stcp:{host}\\{instance}"),
        None => format!("-Stcp:{host},{port}"),
    };
    let sign_in = match auth {
        AuthMode::Password => format!("-U{user}"),
        AuthMode::Windows => "-E".to_string(),
        AuthMode::AzureCli => return Err(Refusal::SqlcmdEntra),
        // Never in force on SQL Server (`AuthMode::offered`), and refused the
        // same way should a hand-made one arrive.
        AuthMode::AwsIam | AuthMode::GcpIam => {
            return Err(auth
                .external_client_refusal()
                .unwrap_or(Refusal::SqlcmdEntra));
        }
    };
    let mut args = vec![address, sign_in];
    if let Some(db) = database.filter(|d| !d.is_empty()) {
        args.push(format!("-d{db}"));
    }
    args.push("-I".to_string());
    match tls.mode {
        SslMode::Disable => args.push("-No".to_string()),
        SslMode::Prefer | SslMode::Require => {
            args.push("-Nm".to_string());
            args.push("-C".to_string());
        }
        SslMode::VerifyCa | SslMode::VerifyFull => {
            if tls.ca_file().is_some() {
                return Err(Refusal::SqlcmdCaFile);
            }
            args.push("-Nm".to_string());
        }
    }
    Ok(args)
}

/// Why a client running *inside WSL* cannot honour `tls`, or `None`.
///
/// The three certificate paths travel as argv or as environment values, and a
/// Windows-shaped one names a file the Linux client cannot open. The failure
/// mode without this check is not a clean error: the client reports a missing
/// file, or — depending on the rung — quietly proceeds unverified. Refusing is
/// the direction a guess about encryption has to fall, the same rule
/// [`crate::connection::SslMode::STRICTEST`] encodes for an unreadable mode.
///
/// A path that is already POSIX-shaped (`/etc/ca.crt`, or a `/mnt/c/…` the user
/// typed themselves) is left alone — this is not a ban on WSL plus TLS, only on
/// handing a Linux process a drive letter.
pub fn wsl_tls_blocker(tls: &crate::connection::Tls) -> Option<Refusal> {
    let paths = [&tls.ca_path, &tls.client_cert_path, &tls.client_key_path];
    paths
        .iter()
        .any(|p| is_windows_path(p))
        .then_some(Refusal::WslCertificatePaths)
}

/// Why a **tunnelled** connection cannot open a CLI session at this TLS rung,
/// or `None`.
///
/// The app's own connect path rewrites the endpoint to `127.0.0.1:<local>` and
/// carries `hostname_override: Some(conn.host)` **in the same step**, with a
/// test that says why: *"Rewriting the endpoint to `127.0.0.1` would have
/// `verify-full` compare a perfectly good certificate against the loopback
/// address and reject it."* The CLI builders were handed the rewritten
/// `Connection` with its `tls` block untouched and no way to say the same
/// thing — `mysql` has no hostname-override option, and `psql` would need the
/// `PGHOST`/`PGHOSTADDR` split — so the client was told to dial the loopback
/// and verify the certificate against it, and refused its own server's
/// certificate. Before the TLS arguments existed at all the client fell back to
/// a non-verifying default and the session opened.
///
/// A refusal rather than a silent downgrade: dropping to `require` would open a
/// session that says TLS and checks nothing, on a connection whose whole point
/// is that it checks. Only the two verifying rungs ask — `require` and below
/// name no certificate, so the loopback address is the address and nothing is
/// compared.
pub fn tunnelled_verify_blocker(tls: &crate::connection::Tls) -> Option<Refusal> {
    use crate::connection::SslMode;
    matches!(tls.mode, SslMode::VerifyCa | SslMode::VerifyFull).then_some(Refusal::TunnelledVerify)
}

/// Is `p` a path only the Windows side can open?
///
/// A drive-letter prefix (`C:\`, `C:/`) or any backslash. Deliberately crude:
/// the question is only ever asked about a path already being handed across the
/// WSL boundary, where both spellings are wrong and neither is a legal POSIX
/// filename in practice.
fn is_windows_path(p: &str) -> bool {
    if p.is_empty() {
        return false;
    }
    if p.contains('\\') {
        return true;
    }
    let b = p.as_bytes();
    b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'/' || b[2] == b'\\')
}

/// `msg` reduced to bytes that are inert inside **both** shells' `echo`.
///
/// **The one place in the app where a string really is handed to a shell.** The
/// two rules at the top of this module say the argv here is executed directly
/// and nothing parses it — true of every other function, and not of the
/// message terminal: it spawns `/bin/sh -c "printf '%s\n' '<msg>'; exec /bin/sh"` on Unix
/// and `cmd.exe /k echo <msg>` on Windows, so the message is **shell source**.
/// A `'` in it closes the quote on Unix; `&`, `|`, `^`, `>` and `%` are syntax
/// to `cmd`. That was harmless while every caller passed a fixed sentence, and
/// stops being harmless the moment one reports a program name out of
/// `terminal.json` or an OS error string back to the user.
///
/// An **allowlist**, for the reason [`is_url_byte`]'s doc gives: filtering every
/// metacharacter of every shell is how one of them comes to be missed. Letters
/// and digits in any script, space and a short punctuation set that neither
/// shell treats as syntax; everything else — every other non-ASCII character
/// (typographic quotes and dashes, fullwidth symbols, direction marks) and every
/// control character, newline among them — becomes a space, and runs of spaces collapse
/// so a stripped run does not leave a gap. The result can be pasted into either
/// command with no quoting beyond what is already there.
///
/// **A letter in any script is kept** because a translated message is made of
/// them — German's `für` printed `f r` — and no shell gives one a meaning. It
/// was once dropped for the console's codepage, but the panel's console is a
/// pseudo-console, which takes `cmd`'s UTF-16 and hands back UTF-8.
///
/// **The backslash is on the list**, so a Windows path in a message stays a
/// path: `cmd` gives it no meaning, `sh` none inside single quotes, and the
/// Unix shell prints the message with `printf '%s\n'` rather than an `echo`
/// that would read `\n` or `\c` in it. Stripped, a restore refusal named
/// `C: Users jonid backup.sql`.
pub fn shell_message_text(msg: &str) -> String {
    let mut out = String::with_capacity(msg.len());
    for ch in msg.chars() {
        let keep = ch.is_alphanumeric() || " .,:;!?-_/()[]+=@#\\".contains(ch);
        let ch = if keep { ch } else { ' ' };
        if ch == ' ' && out.ends_with(' ') {
            continue;
        }
        out.push(ch);
    }
    out.trim().to_string()
}

/// The arguments for `docker` that list the running containers' full ids, one
/// per line — the first of the import's two calls. Fixed: nothing in it came
/// from anywhere.
pub const DOCKER_PS_ARGS: [&str; 3] = ["ps", "--quiet", "--no-trunc"];

/// The arguments for `docker` that describe the containers [`DOCKER_PS_ARGS`]
/// listed, from that call's output — or an empty list when nothing is running.
///
/// **The ids are another program's output on their way back into a process**,
/// which is this module's question even with no shell in between: each line
/// must be a container id as Docker spells one (64 lowercase hex digits with
/// `--no-trunc`, 12 without), and anything else refuses the whole call rather
/// than being dropped, since a `docker` that printed something else is not the
/// program this was written against. `--type=container` keeps an image that
/// happens to share an id prefix out of the answer.
pub fn docker_inspect_args(ps_output: &str) -> Result<Vec<String>, Refusal> {
    let mut args = vec!["inspect".to_string(), "--type=container".to_string()];
    for line in ps_output.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let is_id = matches!(line.len(), 12 | 64)
            && line
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
        if !is_id {
            return Err(Refusal::NotAContainerId);
        }
        args.push(line.to_string());
    }
    Ok(if args.len() == 2 { Vec::new() } else { args })
}

// ── Backup and restore ──────────────────────────────────────────────────────
//
// The database tree's *Back up* and *Restore from file* run the engine's own
// tools in the terminal panel, as *Open in CLI* runs its client: argv only,
// the password in the client's environment variable, never on the command
// line. The file is one the user picked in a dialog, so it is a Windows path
// on Windows — which is why the app runs these native only, never through
// WSL, where the path would name somewhere else.

/// A backup or restore file the user picked, or why it cannot be handed to a
/// client: a line break, which no file name holds and which `mysql`'s
/// `source` would read as the end of its argument and the start of a second
/// command. Not the only such character for that client — its own rule is
/// [`mysql_source_path`]'s.
pub fn backup_file_target(file: &str) -> Result<&str, Refusal> {
    if file.trim().is_empty() || file.contains(['\n', '\r', '\0']) {
        return Err(Refusal::FileNameLineBreak);
    }
    Ok(file)
}

/// `pg_dump`'s argv: the database in the custom format (`-F c`), which
/// `pg_restore` reads selectively and in parallel, to `file`.
pub fn pg_dump_args(
    host: &str,
    port: u16,
    user: &str,
    db: &str,
    file: &str,
    role: Option<&str>,
) -> Result<Vec<String>, Refusal> {
    let db = psql_target(db)?;
    let file = backup_file_target(file)?;
    let mut args: Vec<String> = vec![
        "-h".into(),
        host.into(),
        "-p".into(),
        port.to_string(),
        "-U".into(),
        user.into(),
        "-F".into(),
        "c".into(),
        "-f".into(),
        file.into(),
    ];
    args.extend(role_arg(role)?);
    args.extend(["-d".into(), db.into()]);
    Ok(args)
}

/// `--role=<role>` for `pg_dump` and `pg_restore` — the role init SQL takes
/// ([`init_role`]) — or nothing.
fn role_arg(role: Option<&str>) -> Result<Option<String>, Refusal> {
    match role {
        None => Ok(None),
        Some(r) if r.is_empty() || r.starts_with('-') || r.contains(['\n', '\r', '\0']) => {
            Err(Refusal::RoleUnpassable)
        }
        Some(r) => Ok(Some(format!("--role={r}"))),
    }
}

/// The client and argv that restore `file` into `db`: `psql -f` for a plain
/// `.sql` script (stopping at its first error, so a half-restored database is
/// not reported as done), `pg_restore` for anything else — the custom or
/// directory format `pg_dump` writes — without restoring ownership, which
/// names roles this server may not have.
pub fn pg_restore_args(
    host: &str,
    port: u16,
    user: &str,
    db: &str,
    file: &str,
    role: Option<&str>,
) -> Result<(&'static str, Vec<String>), Refusal> {
    let db = psql_target(db)?;
    let file = backup_file_target(file)?;
    let role_flag = role_arg(role)?;
    let mut args: Vec<String> = vec![
        "-h".into(),
        host.into(),
        "-p".into(),
        port.to_string(),
        "-U".into(),
        user.into(),
        "-d".into(),
        db.into(),
    ];
    if ends_with_ignore_ascii_case(file, ".sql") {
        args.extend(["-v".into(), "ON_ERROR_STOP=1".into()]);
        // psql has no `--role`: the role is set first, in the session the
        // script then runs in (`-c` and `-f` run in order).
        if let Some(r) = role {
            args.extend([
                "-c".into(),
                format!(
                    "SET ROLE {}",
                    crate::export::ident_sql(r, crate::intel::SqlDialect::Postgres)
                ),
            ]);
        }
        args.extend(["-f".into(), file.into()]);
        Ok(("psql", args))
    } else {
        args.extend(role_flag);
        // `--` before a file the user named, as before a server-supplied name.
        args.extend(["--no-owner".into(), "--".into(), file.into()]);
        Ok(("pg_restore", args))
    }
}

/// `mysqldump`'s argv for `db` to `file`: one consistent snapshot of the
/// transactional tables (`--single-transaction`, no lock held), and the
/// routines, triggers and events with them. `tls` is
/// [`mysql_cli_tls_args`]' answer for the client found, as *Open in CLI*
/// passes it; `--` before the database name, which the server supplied.
pub fn mysqldump_args(
    host: &str,
    port: u16,
    user: &str,
    tls: Vec<String>,
    db: &str,
    file: &str,
) -> Result<Vec<String>, Refusal> {
    let file = backup_file_target(file)?;
    let mut args: Vec<String> = vec![
        "-h".into(),
        host.into(),
        "-P".into(),
        port.to_string(),
        "-u".into(),
        user.into(),
    ];
    args.extend(tls);
    args.extend([
        "--single-transaction".into(),
        "--routines".into(),
        "--triggers".into(),
        "--events".into(),
        format!("--result-file={file}"),
        "--".into(),
        db.into(),
    ]);
    Ok(args)
}

/// The `mysql` client's argv restoring `file` into `db`: its `source` command,
/// which runs a script the way a dump expects (its `DELIMITER` lines
/// included), reading the file itself — there is no shell to redirect one in.
pub fn mysql_restore_args(
    host: &str,
    port: u16,
    user: &str,
    tls: Vec<String>,
    db: &str,
    file: &str,
) -> Result<Vec<String>, Refusal> {
    let file = mysql_source_path(file, cfg!(windows))?;
    let mut args: Vec<String> = vec![
        "-h".into(),
        host.into(),
        "-P".into(),
        port.to_string(),
        "-u".into(),
        user.into(),
    ];
    args.extend(tls);
    args.extend([format!("--execute=source {file}"), "--".into(), db.into()]);
    Ok(args)
}

/// The T-SQL that backs `db` up on SQL Server: to `file`, a bare name, which
/// the server places in **its own** default backup folder — the backup is
/// written by the server, so a path on this machine would mean nothing to it.
/// `COPY_ONLY`, so it does not reset the differential base a scheduled backup
/// plan relies on; `INIT`, so a file of that name is replaced rather than
/// appended to.
pub fn mssql_backup_sql(db: &str, file: &str) -> String {
    use crate::export::{ident_sql, sql_literal};
    use crate::intel::SqlDialect;
    format!(
        "BACKUP DATABASE {} TO DISK = {} WITH COPY_ONLY, INIT",
        ident_sql(db, SqlDialect::MsSql),
        sql_literal(
            &crate::model::Value::Str(file.to_string()),
            SqlDialect::MsSql
        )
    )
}

/// The MySQL-family dump tools *Back up* looks for, in order. **MariaDB's
/// name first**: where MariaDB's clients are installed (Debian's and Ubuntu's
/// default) `mysqldump` is a symlink to `mariadb-dump`, so found by that name
/// it read as Oracle's tool ([`MysqlClient::of`]) and was handed
/// `--ssl-mode`, which it refuses — every backup failed. No Oracle install
/// ships a `mariadb-dump`, so asking for it first costs nothing.
pub const MYSQL_DUMP_TOOLS: [&str; 2] = ["mariadb-dump", "mysqldump"];

/// The clients *Restore from file* looks for, in order — `mariadb` before
/// `mysql`, for [`MYSQL_DUMP_TOOLS`]' reason.
pub const MYSQL_RESTORE_TOOLS: [&str; 2] = ["mariadb", "mysql"];

/// Does *Back up* on `dialect` write **on the server**, so that a read-only
/// connection may not take one? SQL Server's is a `BACKUP DATABASE` the
/// server runs ([`mssql_backup_sql`]) — a file in its own folder, overwritten
/// by `INIT`, and history rows in `msdb`. Every other engine's is a read the
/// tool writes to a file on this machine, as an export is. Exhaustive, so a
/// fifth engine answers here.
pub fn backup_writes_server(dialect: crate::intel::SqlDialect) -> bool {
    use crate::intel::SqlDialect;
    match dialect {
        SqlDialect::MsSql => true,
        SqlDialect::MySql | SqlDialect::Postgres | SqlDialect::Sqlite => false,
    }
}

/// The role a connection's init SQL takes with a plain `SET ROLE <name>` —
/// the last such statement — for the PostgreSQL tools' `--role`, or `None`.
///
/// **Backup and restore run outside the app's own connections**, so the init
/// SQL every one of those runs never reaches `pg_dump`, `pg_restore` or
/// `psql`: a login with no grants of its own and `SET ROLE app_owner` in its
/// init SQL backed up as itself (permission denied) and restored objects
/// owned by itself. The role is the one setting there that decides *whose*
/// the work is, and the tools take it as an option. Only PostgreSQL's tools
/// have one; `SET ROLE NONE` is no role, and a statement init SQL refuses is
/// not read.
pub fn init_role(init_sql: &str, dialect: crate::intel::SqlDialect) -> Option<String> {
    use crate::intel::SqlDialect;
    match dialect {
        SqlDialect::Postgres => {}
        SqlDialect::MySql | SqlDialect::MsSql | SqlDialect::Sqlite => return None,
    }
    let statements = crate::init_sql::check(init_sql, dialect).ok()?;
    let mut role = None;
    for s in &statements {
        let words = crate::sql::leading_words(s, 2, dialect);
        if words.first().map(String::as_str) != Some("SET")
            || words.get(1).map(String::as_str) != Some("ROLE")
        {
            continue;
        }
        let mut end = 0usize;
        for _ in 0..2 {
            end += crate::sql::leading_keyword_end(&s[end..], dialect)?;
        }
        let (name, after) = crate::sql::ident_at(s, end, dialect);
        // Nothing but the name after it, bar the terminator.
        let rest = s[after..].trim().trim_end_matches(';').trim();
        // **As the server reads it**: a bare name folded where the engine
        // folds one (`AppOwner` is `appowner`), a quoted one as written. The
        // tools quote what they are handed (`fmtId`), so the spelling given
        // them must be the role's own.
        let quoted = s[end..]
            .trim_start()
            .bytes()
            .next()
            .is_some_and(|b| crate::intel::ident_quote(dialect, b).is_some());
        role = match name {
            Some(n) if rest.is_empty() && !n.eq_ignore_ascii_case("NONE") => {
                Some(if !quoted && dialect.folds_unquoted_names() {
                    n.to_ascii_lowercase()
                } else {
                    n
                })
            }
            _ => None,
        };
    }
    role
}

/// The restore file as the `mysql` client's `source` command takes it, or
/// why it cannot.
///
/// **The path is spliced into the client's command line**, and the client
/// reads `source …` as a command only when the line holds neither its
/// delimiter `;` nor `\g`; otherwise the line goes to its parser, where `;`
/// ends the path, `\g` sends what came before to the server as SQL, and on
/// Unix `\!` runs a shell command — measured on MariaDB 10.11's client with
/// a name holding `\! touch …;`. So a path with either is refused, and so is
/// one with space at an end, which the client trims. On `windows` every path
/// is backslashed (and `C:\Users\me\git\` holds a `\g`); the client takes `/`
/// there, so the separators are turned round first.
pub fn mysql_source_path(file: &str, windows: bool) -> Result<String, Refusal> {
    let file = backup_file_target(file)?;
    let path = if windows {
        file.replace('\\', "/")
    } else {
        file.to_string()
    };
    if path.contains([';', '\\']) || path.trim() != path {
        return Err(Refusal::MysqlSourcePath);
    }
    Ok(path)
}

/// Does restoring `file` on `dialect` hand it to a client **as a script** —
/// one whose `USE` or `\connect` can move it off the database it was pointed
/// at, so it is read first (`script::database_switches`)? MySQL's restore
/// always `source`s the file; PostgreSQL's runs a `.sql` through `psql` and
/// anything else through `pg_restore`, which restores into `-d` whatever the
/// archive says unless asked to create its database ([`pg_restore_args`]).
pub fn restore_runs_a_script(file: &str, dialect: crate::intel::SqlDialect) -> bool {
    use crate::intel::SqlDialect;
    match dialect {
        SqlDialect::MySql => true,
        SqlDialect::Postgres => ends_with_ignore_ascii_case(file, ".sql"),
        SqlDialect::MsSql | SqlDialect::Sqlite => false,
    }
}

/// The write-log line *Restore from file* writes as it launches the client:
/// the file and the database in `detail`, the outcome
/// [`Unknown`](crate::write_log::Outcome::Unknown) — the terminal's client, not
/// Schemaic, sees how it ends. Without it the largest write the app can start
/// left nothing in a log that claims every write.
pub fn restore_log_entry(connection: &str, db: &str, file: &str) -> crate::write_log::Entry {
    crate::write_log::Entry::now(crate::write_log::Source::Restore, connection, Some(db), "")
        .with_detail(format!("restore {file} into {db}, run in the terminal"))
        .ended_as(crate::write_log::Outcome::Unknown, None)
}

/// Is *Restore from file* offered on `dialect`? Where a client restores a
/// file from this machine into a database: `psql`/`pg_restore` and `mysql`.
/// Not SQL Server, whose `RESTORE` reads a file **on the server** and needs
/// the database to itself, nor SQLite, where a backup is a database file one
/// opens as a connection. Exhaustive, so a fifth engine answers here.
pub fn restore_offered(dialect: crate::intel::SqlDialect) -> bool {
    use crate::intel::SqlDialect;
    match dialect {
        SqlDialect::MySql | SqlDialect::Postgres => true,
        SqlDialect::MsSql | SqlDialect::Sqlite => false,
    }
}

/// The local time as a backup file's name spells it: `20261014-1530`.
pub fn backup_stamp_now() -> String {
    chrono::Local::now().format("%Y%m%d-%H%M").to_string()
}

/// A backup's default file name: the database, a timestamp and `ext`, with
/// anything a file name cannot hold replaced by `_`.
pub fn backup_file_name(db: &str, stamp: &str, ext: &str) -> String {
    let safe: String = db
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!("{safe}_{stamp}.{ext}")
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Backup and restore ──────────────────────────────────────────────────

    #[test]
    fn pg_dump_writes_the_custom_format_to_the_picked_file() {
        let args = pg_dump_args("h", 5432, "u", "shop", "C:/b/shop.dump", None).unwrap();
        assert_eq!(
            args.join(" "),
            "-h h -p 5432 -U u -F c -f C:/b/shop.dump -d shop"
        );
        assert!(
            pg_dump_args("h", 5432, "u", "host=evil", "f", None).is_err(),
            "psql_target's rule"
        );
    }

    #[test]
    fn a_sql_script_restores_through_psql_and_a_dump_through_pg_restore() {
        let (prog, args) = pg_restore_args("h", 1, "u", "shop", "x.SQL", None).unwrap();
        assert_eq!(prog, "psql");
        assert!(args.ends_with(&[
            "-v".into(),
            "ON_ERROR_STOP=1".into(),
            "-f".into(),
            "x.SQL".into()
        ]));
        let (prog, args) = pg_restore_args("h", 1, "u", "shop", "-x.dump", None).unwrap();
        assert_eq!(prog, "pg_restore");
        assert!(
            args.ends_with(&["--no-owner".into(), "--".into(), "-x.dump".into()]),
            "{args:?}"
        );
    }

    #[test]
    fn mysqldump_snapshots_with_routines_and_ends_options_before_the_name() {
        let args = mysqldump_args(
            "h",
            3306,
            "u",
            vec!["--ssl-mode=REQUIRED".into()],
            "--evil",
            "o.sql",
        )
        .unwrap();
        assert_eq!(
            args.join(" "),
            "-h h -P 3306 -u u --ssl-mode=REQUIRED --single-transaction --routines --triggers \
             --events --result-file=o.sql -- --evil"
        );
    }

    #[test]
    fn a_mysql_restore_sources_the_file_and_refuses_a_line_break() {
        let args = mysql_restore_args("h", 1, "u", vec![], "shop", "C:/a b/x.sql").unwrap();
        assert!(args.contains(&"--execute=source C:/a b/x.sql".to_string()));
        assert_eq!(&args[args.len() - 2..], ["--", "shop"]);
        assert!(
            mysql_restore_args("h", 1, "u", vec![], "shop", "x.sql\nDROP DATABASE shop").is_err()
        );
        assert!(backup_file_target("  ").is_err());
    }

    /// **The path is spliced into the `mysql` client's command line**, and
    /// the client reads `source` as a named command only when the line holds
    /// neither its delimiter nor `\g`: past either, the line goes to its
    /// parser, where `;` ends the path, `\g` sends what came before it to the
    /// server, and on Unix `\!` runs a shell (all three measured on MariaDB
    /// 10.11's client). On Windows every path has backslashes, and `C:\…\git\`
    /// is a `\g`; the client takes `/` there, so they are turned round.
    #[test]
    fn a_mysql_restore_refuses_a_path_its_client_would_parse() {
        for bad in [
            "/tmp/a;b/x.sql",
            "/tmp/x\\git/y.sql",
            "/tmp/\\! id;.sql",
            " x.sql",
        ] {
            assert!(mysql_source_path(bad, false).is_err(), "{bad:?}");
        }
        assert!(mysql_restore_args("h", 1, "u", vec![], "shop", "/tmp/a;b/x.sql").is_err());
        assert!(mysql_restore_args("h", 1, "u", vec![], "shop", "\\! id;.sql").is_err());
        assert_eq!(
            mysql_source_path("C:\\Users\\me\\git\\x.sql", true).as_deref(),
            Ok("C:/Users/me/git/x.sql")
        );
        assert!(mysql_source_path("C:\\a;b\\x.sql", true).is_err());
        assert_eq!(
            mysql_source_path("/srv/a b/x.sql", false).as_deref(),
            Ok("/srv/a b/x.sql")
        );
    }

    /// **`mariadb-dump` and `mariadb` first.** On a machine with MariaDB's
    /// clients (Debian's and Ubuntu's default) `mysqldump` and `mysql` are
    /// symlinks to them, so the name read as Oracle's and the argv carried
    /// `--ssl-mode`, which they refuse — every backup and restore failed at
    /// once (measured with 10.11). No Oracle install ships a `mariadb-dump`,
    /// so trying it first breaks nothing.
    #[test]
    fn mariadbs_tools_are_tried_before_the_names_they_share() {
        assert_eq!(MYSQL_DUMP_TOOLS[0], "mariadb-dump");
        assert_eq!(MYSQL_RESTORE_TOOLS[0], "mariadb");
        assert_eq!(MysqlClient::of(MYSQL_DUMP_TOOLS[0]), MysqlClient::MariaDb);
        assert_eq!(
            MysqlClient::of(MYSQL_RESTORE_TOOLS[0]),
            MysqlClient::MariaDb
        );
        assert!(MYSQL_DUMP_TOOLS.contains(&"mysqldump") && MYSQL_RESTORE_TOOLS.contains(&"mysql"));
    }

    /// SQL Server's backup is a `BACKUP DATABASE` the server runs and writes —
    /// a file in its folder and rows in `msdb` — so a read-only connection may
    /// not take one; every other engine's is a file on this machine.
    #[test]
    fn only_sql_servers_backup_writes_on_the_server() {
        use crate::intel::SqlDialect;
        assert!(backup_writes_server(SqlDialect::MsSql));
        for d in [SqlDialect::MySql, SqlDialect::Postgres, SqlDialect::Sqlite] {
            assert!(!backup_writes_server(d), "{d:?}");
        }
    }

    /// **The role init SQL assumes is the tools' too.** A login with no grants
    /// of its own and `SET ROLE app_owner` in its init SQL backed up as itself
    /// (permission denied) and restored objects owned by itself.
    #[test]
    fn a_plain_set_role_in_init_sql_is_the_tools_role() {
        use crate::intel::SqlDialect::{MySql, Postgres};
        assert_eq!(
            init_role("SET ROLE app_owner", Postgres).as_deref(),
            Some("app_owner")
        );
        assert_eq!(
            init_role("SET search_path = app;\nSET ROLE \"App Owner\";", Postgres).as_deref(),
            Some("App Owner")
        );
        // **Unquoted, the server folds it**, and the tools quote what they are
        // given (`fmtId`): `--role=AppOwner` asked for `"AppOwner"`, a role
        // that does not exist, while the app's own connections, folding it,
        // worked (R2-L6-01).
        assert_eq!(
            init_role("SET ROLE AppOwner", Postgres).as_deref(),
            Some("appowner")
        );
        assert_eq!(init_role("SET ROLE NONE", Postgres), None);
        assert_eq!(init_role("SET search_path = app", Postgres), None);
        assert_eq!(init_role("", Postgres), None);
        assert_eq!(
            init_role("SET ROLE r", MySql),
            None,
            "the PostgreSQL tools' option"
        );

        let args = pg_dump_args("h", 1, "u", "shop", "f.dump", Some("app_owner")).unwrap();
        assert!(args.contains(&"--role=app_owner".to_string()), "{args:?}");
        let (_, args) = pg_restore_args("h", 1, "u", "shop", "f.dump", Some("app_owner")).unwrap();
        assert!(args.contains(&"--role=app_owner".to_string()), "{args:?}");
        // psql has no `--role`: the role is set first, in the same session.
        let (_, args) = pg_restore_args("h", 1, "u", "shop", "f.sql", Some("App Owner")).unwrap();
        let at = args.iter().position(|a| a == "-c").expect("a -c");
        assert_eq!(args[at + 1], "SET ROLE \"App Owner\"");
        assert!(at < args.iter().position(|a| a == "-f").unwrap());
        assert!(pg_dump_args("h", 1, "u", "shop", "f", Some("-x")).is_err());
    }

    /// A restore is logged as it launches — its file and database said, and
    /// its outcome not claimed, since the terminal's client is what sees it.
    /// Every file `mysql` sources and every `.sql` psql runs is read for a
    /// database switch; a PostgreSQL archive goes to `pg_restore`, which the
    /// same rule in `pg_restore_args` decides.
    #[test]
    fn the_restores_that_run_a_script_are_the_ones_read_first() {
        use crate::intel::SqlDialect;
        assert!(restore_runs_a_script("x.dump", SqlDialect::MySql));
        assert!(restore_runs_a_script("x.SQL", SqlDialect::Postgres));
        assert!(!restore_runs_a_script("x.dump", SqlDialect::Postgres));
        for f in ["x.sql", "x.dump"] {
            let (tool, _) = pg_restore_args("h", 1, "u", "d", f, None).unwrap();
            assert_eq!(
                tool == "psql",
                restore_runs_a_script(f, SqlDialect::Postgres),
                "{f}"
            );
        }
    }

    #[test]
    fn a_restore_is_logged_as_launched_with_its_outcome_unknown() {
        use crate::write_log::{Outcome, Source};
        let e = restore_log_entry("prod", "shop", "/b/shop.sql");
        assert_eq!(e.source, Source::Restore);
        assert_eq!(e.outcome, Outcome::Unknown);
        assert_eq!(e.database.as_deref(), Some("shop"));
        assert_eq!(e.connection, "prod");
        let detail = e.detail.unwrap_or_default();
        assert!(
            detail.contains("/b/shop.sql") && detail.contains("shop"),
            "{detail}"
        );
    }

    #[test]
    fn a_sql_server_backup_is_quoted_and_leaves_the_backup_chain_alone() {
        assert_eq!(
            mssql_backup_sql("o]rders", "o_1.bak"),
            "BACKUP DATABASE [o]]rders] TO DISK = N'o_1.bak' WITH COPY_ONLY, INIT"
        );
    }

    #[test]
    fn mariadbs_dump_tool_spells_tls_as_its_client_does() {
        assert_eq!(MysqlClient::of("mariadb-dump"), MysqlClient::MariaDb);
        assert_eq!(MysqlClient::of("mysqldump"), MysqlClient::Mysql);
    }

    #[test]
    fn restore_is_offered_where_a_client_restores_a_local_file() {
        use crate::intel::SqlDialect;
        assert!(restore_offered(SqlDialect::MySql) && restore_offered(SqlDialect::Postgres));
        assert!(!restore_offered(SqlDialect::MsSql) && !restore_offered(SqlDialect::Sqlite));
    }

    #[test]
    fn a_backup_file_name_is_safe_whatever_the_database_is_called() {
        assert_eq!(
            backup_file_name("sales/2026 q1", "20261014-1530", "dump"),
            "sales_2026_q1_20261014-1530.dump"
        );
    }
    use crate::connection::{SslMode, Tls};

    #[test]
    fn docker_inspect_takes_the_ids_docker_ps_printed() {
        let a = "a".repeat(64);
        let b = format!("{}0123", "f".repeat(60));
        let args = docker_inspect_args(&format!("{a}\r\n{b}\n\n")).expect("valid ids");
        assert_eq!(
            args,
            ["inspect", "--type=container", a.as_str(), b.as_str()]
        );
        // The short spelling is an id too.
        assert_eq!(
            docker_inspect_args("0123456789ab").expect("short id").len(),
            3
        );
    }

    #[test]
    fn nothing_running_is_no_call_at_all() {
        assert_eq!(docker_inspect_args(""), Ok(Vec::new()));
        assert_eq!(docker_inspect_args("  \n\n"), Ok(Vec::new()));
    }

    #[test]
    fn a_line_that_is_not_a_container_id_refuses_the_call() {
        for bad in [
            "--format={{.Config.Env}}",
            "abc",
            &"A".repeat(64),
            &"g".repeat(64),
            &"a".repeat(65),
            "0123456789ab cdef",
        ] {
            assert!(docker_inspect_args(bad).is_err(), "{bad:?} was accepted");
        }
    }

    /// **The message terminal is the one place a string reaches a shell**, and
    /// its callers stopped being fixed sentences the moment one reported a
    /// program name out of `terminal.json`.
    #[test]
    fn a_shell_message_carries_no_syntax_of_either_shell() {
        // Unix: the message sits inside single quotes in `sh -c`, so the one
        // byte that matters is `'`. With it gone the `;` cannot start a
        // command — it is inside the app's quotes, and nothing in the message
        // can close them.
        assert_eq!(
            shell_message_text("Couldn't start '; rm -rf ~; echo '"),
            "Couldn t start ; rm -rf ; echo"
        );
        assert!(!shell_message_text("a'b").contains('\''));
        // Windows: `cmd /k echo` reads these.
        for m in ["a & b", "a | b", "a ^ b", "a > b", "a %PATH% b", "a < b"] {
            let out = shell_message_text(m);
            assert!(
                !out.contains(['&', '|', '^', '>', '<', '%', '\'', '"', '`', '$']),
                "{m} -> {out}"
            );
        }
        // A newline would end the command and start another — a bare LF and a
        // Windows CRLF alike. (These were two copies of the LF case, written as
        // raw line breaks in the source, so the CRLF one was never tested.)
        assert_eq!(shell_message_text("one\ntwo"), "one two");
        assert_eq!(shell_message_text("one\r\ntwo"), "one two");
        // Ordinary text survives, including the punctuation a real sentence
        // needs, and a stripped run leaves one space rather than a gap.
        assert_eq!(
            shell_message_text(
                "Couldn't start /usr/bin/zsh (No such file or directory). Pick another shell in terminal settings."
            ),
            "Couldn t start /usr/bin/zsh (No such file or directory). Pick another shell in terminal settings."
        );
        // **A letter in any script stays** — no shell reads one as syntax, and a
        // translated message is made of them: German's `für` printed `f r`.
        // Every other non-ASCII character still goes, the typographic quotes
        // and dashes among them, along with the marks no allowlist vouches for.
        assert_eq!(shell_message_text("naïve 東京"), "naïve 東京");
        assert_eq!(
            shell_message_text("Kein Client für „mysql“ — öffne"),
            "Kein Client für mysql öffne"
        );
        assert_eq!(shell_message_text("a\u{202E}b \u{FF06} c"), "a b c");
        assert_eq!(shell_message_text("   "), "");
        assert_eq!(shell_message_text(""), "");
        // **A Windows path stays legible**: a backslash is no syntax to `cmd`,
        // and inside `sh`'s single quotes nothing escapes — the Unix branch
        // prints with `printf '%s\n'`, so no `echo` reads `\n` in it either. A
        // restore refusal once named `C: Users jonid backup.sql`.
        assert_eq!(
            shell_message_text(r"C:\Users\me\new.sql moves to another database"),
            r"C:\Users\me\new.sql moves to another database"
        );
        assert!(!shell_message_text(r"a\'b").contains('\''));
    }

    /// The programs that would make any amount of filtering above pointless.
    const SHELLS: &[&str] = &[
        "cmd",
        "cmd.exe",
        "sh",
        "bash",
        "zsh",
        "powershell",
        "powershell.exe",
        "pwsh",
        "start",
    ];

    /// **Every launcher this module may name, and whether it is itself an
    /// interpreter** — `(program, is_a_script)`.
    ///
    /// A denylist of nine shell *names* cannot say anything about a program
    /// that is not one of them, which is how `xdg-open` — a `#!/bin/sh` script
    /// shipped by `xdg-utils`, and the one interpreter in this set — passed
    /// `no_shell_ever_reads_a_url` while the module header claimed no shell was
    /// ever in between. An allowlist makes a new launcher a thing that has to
    /// be *classified* rather than merely absent from a list of shells, and
    /// `a_launcher_that_is_an_interpreter_is_declared` is what holds the second
    /// column to the header's own paragraph.
    const LAUNCHERS: &[(&str, bool)] = &[
        ("explorer", false),
        ("open", false),
        // See rule 1 in the module header for what this costs and what is
        // still guaranteed.
        ("xdg-open", true),
    ];

    /// The composition, not the predicate: a hostile URL must either be refused
    /// outright **or** reach a program that is not a shell, as exactly one argv
    /// element. Red against `cmd /C start "" <url>`, and it stays red for any
    /// future launcher that reintroduces a shell however well the gate filters.
    #[test]
    fn no_shell_ever_reads_a_url() {
        let candidates = [
            "https://example.com/a&calc",
            "https://example.com/a&%windir%/system32/calc.exe",
            "https://example.com/a|calc",
            "https://example.com/a^&calc",
            "https://example.com/?a=1&b=2",
            "http://example.com/",
        ];
        for raw in candidates {
            let Some(argv) = url_open_argv(raw) else {
                continue;
            };
            assert!(
                !SHELLS.contains(&argv[0].as_str()),
                "{raw} is launched through a shell: {argv:?}"
            );
            assert_eq!(argv.len(), 2, "{raw} did not travel as one argv element");
            assert_eq!(argv[1], raw);
            // **And it is a launcher this module has declared**, which is the
            // half the shell-name list could not do: a program that is not one
            // of the nine names above says nothing about whether it parses its
            // argument. See `LAUNCHERS`.
            assert!(
                LAUNCHERS.iter().any(|(p, _)| *p == argv[0]),
                "{raw} is launched through `{}`, which is not in `LAUNCHERS` — \
                 add it there with an honest answer to \"is this itself an \
                 interpreter\", and amend rule 1 in the module header if it is",
                argv[0]
            );
        }
    }

    /// **The header's exception and the table agree.**
    ///
    /// Rule 1 names `xdg-open` as the one launcher that is itself a shell
    /// script. If a future edit adds a second interpreter to `LAUNCHERS`
    /// without amending that paragraph, the module's stated guarantee and its
    /// actual behaviour part company again — which is the whole of this
    /// finding, one launcher earlier.
    #[test]
    fn a_launcher_that_is_an_interpreter_is_declared() {
        let src = include_str!("launch.rs");
        let header: String = src
            .lines()
            .take_while(|l| l.starts_with("//!") || l.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        for (program, is_script) in LAUNCHERS {
            if !*is_script {
                continue;
            }
            assert!(
                header.contains(program),
                "`{program}` is an interpreter and the module header does not \
                 say so — rule 1 claims no shell is ever in between"
            );
        }
        // And the floor: the exception is still one, so this is not passing by
        // finding no interpreters at all.
        assert_eq!(
            LAUNCHERS.iter().filter(|(_, s)| *s).count(),
            1,
            "the number of interpreters in the launcher set changed — rule 1 \
             names exactly one, so amend it rather than this number alone"
        );
    }

    #[test]
    fn a_real_query_string_still_opens() {
        assert_eq!(
            openable_url("https://example.com/?a=1&b=2"),
            Some("https://example.com/?a=1&b=2")
        );
        assert!(url_open_argv("https://github.com/search?q=x&type=code").is_some());
        // The sub-delims that are syntax to `sh` but legal in a URI, kept on
        // purpose because nothing downstream is a shell.
        assert!(url_open_argv("https://example.com/?a=$1;b='x'&c=(2)").is_some());
    }

    #[test]
    fn the_scheme_is_required_and_case_insensitive() {
        assert_eq!(
            openable_url("HTTPS://example.com"),
            Some("HTTPS://example.com")
        );
        assert_eq!(
            openable_url("Http://example.com"),
            Some("Http://example.com")
        );
        assert_eq!(openable_url("file:///etc/passwd"), None);
        assert_eq!(openable_url("javascript:alert(1)"), None);
        assert_eq!(openable_url("example.com"), None);
        assert_eq!(openable_url(""), None);
        assert_eq!(openable_url("https://"), None, "a scheme with no authority");
    }

    #[test]
    fn shell_syntax_and_control_bytes_are_refused() {
        for raw in [
            "https://example.com/a\"b",
            "https://example.com/a<b",
            "https://example.com/a>b",
            "https://example.com/a^b",
            "https://example.com/a|b",
            "https://example.com/a\\b",
            "https://example.com/a`b",
            "https://example.com/a{b}",
            "https://example.com/a b",
            "https://example.com/a\nb",
            "https://example.com/a\rb",
            "https://example.com/a\tb",
            "https://example.com/a\0b",
            "https://example.com/\u{00e9}",
        ] {
            assert_eq!(openable_url(raw), None, "{raw:?} was accepted");
        }
    }

    #[test]
    fn percent_encoding_must_be_well_formed() {
        assert!(openable_url("https://example.com/a%20b").is_some());
        assert!(openable_url("https://example.com/%E2%9C%93").is_some());
        assert_eq!(openable_url("https://example.com/%windir%/x"), None);
        assert_eq!(openable_url("https://example.com/100%"), None);
        assert_eq!(openable_url("https://example.com/%2"), None);
        assert_eq!(openable_url("https://example.com/%zz"), None);
    }

    #[test]
    fn a_conninfo_shaped_database_name_is_refused_for_psql() {
        assert!(psql_target("dbname=postgres host=192.0.2.1").is_err());
        assert!(psql_target("a=b").is_err());
        assert!(psql_target("postgresql://evil.example.com/x").is_err());
        assert!(psql_target("POSTGRES://evil.example.com/x").is_err());
    }

    /// The SQLite arm of the same family, which had no guard at all while both
    /// its siblings got one. `sqlite3` reads a leading `-` as an option, so a
    /// file the user could plausibly hold — or one carried in from a DBeaver /
    /// HeidiSQL import — opened a session on something other than the file the
    /// schema tree was showing.
    #[test]
    fn a_dash_leading_database_file_is_refused_for_sqlite() {
        assert!(sqlite_target("-tmp.db").is_err());
        assert!(sqlite_target("--help").is_err());
        assert!(sqlite_target("-").is_err());
    }

    /// And everything else still reaches the client, including the shapes a
    /// shell would care about and this does not — there is no shell here.
    #[test]
    fn an_ordinary_database_file_still_reaches_sqlite() {
        for f in [
            "shop.db",
            "C:/Users/a b/My Documents/shop.db",
            "/home/u/it's mine.db",
            "./-tmp.db",
            "data/-tmp.db",
            "a;rm -rf x.db",
            "",
        ] {
            assert_eq!(sqlite_target(f), Ok(f), "{f} was refused");
        }
    }

    #[test]
    fn an_ordinary_database_name_still_reaches_psql() {
        for db in ["postgres", "my_app", "Orders-2026", "a b", "--pager", "x'y"] {
            assert_eq!(psql_target(db), Ok(db), "{db} was refused");
        }
    }

    /// **A length guard is not a char-boundary guard.** Both prefix tests here
    /// proved the slice was in *range* and then indexed a `&str` at a fixed byte
    /// count — which panics when that byte is inside a character. The name comes
    /// off `pg_database.datname` and is arbitrary UTF-8, and this runs on the
    /// Floem UI thread from a click handler, so the panic took the app rather
    /// than reaching the panel as every other refusal on this path does.
    ///
    /// `postgres:/é` is 12 bytes and `postgres://` is 11, so the length guard
    /// passed and byte 11 is `é`'s continuation byte. `https:/é` is the same
    /// arithmetic one function up.
    #[test]
    fn a_name_whose_character_straddles_the_prefix_is_answered_not_panicked_on() {
        assert_eq!(psql_target("postgres:/\u{e9}"), Ok("postgres:/\u{e9}"));
        assert_eq!(
            psql_target("postgresql:/\u{e9}x"),
            Ok("postgresql:/\u{e9}x")
        );
        assert_eq!(strip_scheme("https:/\u{e9}"), None);
        assert_eq!(strip_scheme("http:/\u{e9}"), None);
        // And the composition, since a boundary-safe predicate and a hostile
        // caller are two different subjects: the launcher answers a `Result`.
        assert!(openable_url("https:/\u{e9}").is_none());
    }

    /// **A `.cmd`/`.bat` client puts `cmd.exe` between the argv and the
    /// process**, and `cmd` does not know what `--` means.
    ///
    /// The whole DB-CLI path rests on this module's rule 1 — "no shell is ever
    /// in between" — which is what licenses `mysql_shell_config` to pass a
    /// server-supplied database name through behind a `--` terminator at all.
    /// On Windows that premise is false for a batch image, and a `.cmd` shim on
    /// `PATH` is a supported configuration: `shell::which`'s own `PATHEXT`
    /// default names `.CMD` and `.BAT`.
    #[test]
    fn a_batch_shim_is_refused_rather_than_spawned() {
        use std::path::Path;
        for shim in [
            r"C:	ools\mysql.cmd",
            r"C:	ools\mariadb.BAT",
            "/opt/bin/psql.Cmd",
        ] {
            assert!(
                direct_spawn_verdict(Path::new(shim)).is_err(),
                "{shim} runs through cmd.exe"
            );
        }
        // A real image is spawned directly, extension or none.
        for real in [
            r"C:\Program Files\MySQLin\mysql.exe",
            "/usr/bin/psql",
            r"C:	ools\sqlite3.com",
        ] {
            assert!(direct_spawn_verdict(Path::new(real)).is_ok(), "{real}");
        }
        // And a directory named `.cmd` is not the program's extension.
        assert!(direct_spawn_verdict(Path::new("/opt/x.cmd/psql")).is_ok());
    }

    /// The same arithmetic at the other end of the string — the extension test
    /// `pick_executable` and `batch_argv_refused` both asked by slicing.
    #[test]
    fn the_extension_test_answers_a_non_ascii_name() {
        // Red against the slicing spelling: the last 3 bytes of each of these
        // land inside a character.
        assert!(!ends_with_ignore_ascii_case("na\u{ef}ve", ".EXE"));
        assert!(!ends_with_ignore_ascii_case("\u{65e5}\u{672c}", ".cmd"));
        assert!(!ends_with_ignore_ascii_case(
            "\u{514b}\u{52b3}\u{5fb7}",
            ".bat"
        ));
        // And it still answers the question it was written for.
        assert!(ends_with_ignore_ascii_case("opencode.CMD", ".cmd"));
        assert!(ends_with_ignore_ascii_case("caf\u{e9}.exe", ".EXE"));
        assert!(!ends_with_ignore_ascii_case(".cmd", ".cmd"), "not a shim");
        assert!(!ends_with_ignore_ascii_case("x.exe", ".cmd"));
    }

    /// `--pager` is only dangerous as an *option*, and it is only an option
    /// because of where it sits in the argv — which is why the MySQL fix is a
    /// `--` terminator in the builder and not a name filter here.
    #[test]
    fn the_mysql_client_gets_a_mode_flag_for_every_rung() {
        let args = |mode| {
            mysql_cli_tls_args(
                &Tls {
                    mode,
                    ..Tls::default()
                },
                MysqlClient::Mysql,
            )
        };
        assert_eq!(args(SslMode::Disable), ["--ssl-mode=DISABLED"]);
        assert_eq!(args(SslMode::Prefer), ["--ssl-mode=PREFERRED"]);
        assert_eq!(args(SslMode::Require), ["--ssl-mode=REQUIRED"]);
        assert_eq!(args(SslMode::VerifyCa), ["--ssl-mode=VERIFY_CA"]);
        assert_eq!(args(SslMode::VerifyFull), ["--ssl-mode=VERIFY_IDENTITY"]);
        for mode in SslMode::ALL {
            assert!(
                !args(mode).is_empty(),
                "{mode:?} would leave the client on its PREFERRED default"
            );
        }
    }

    /// **MariaDB's client has no `--ssl-mode`**, and the flag was emitted for
    /// it unconditionally — at every rung, the default included. Its
    /// `my_getopt` exits immediately with `unknown option
    /// '--ssl-mode=DISABLED'`, so the terminal panel showed that instead of a
    /// session: the feature broken outright for this project's own primary
    /// engine and its own live tier.
    ///
    /// `resolve_cli` accepts either binary, so the argv has to ask which one it
    /// is building for. A `mysql` that is really a MariaDB symlink stays
    /// ambiguous — see `MysqlClient::of`.
    #[test]
    fn the_mariadb_client_gets_its_own_spelling() {
        let args = |mode| {
            mysql_cli_tls_args(
                &Tls {
                    mode,
                    ..Tls::default()
                },
                MysqlClient::MariaDb,
            )
        };
        for mode in SslMode::ALL {
            assert!(
                !args(mode).iter().any(|a| a.starts_with("--ssl-mode")),
                "{mode:?} sends an option this client does not have"
            );
        }
        assert_eq!(args(SslMode::Disable), ["--skip-ssl"]);
        // **What `Prefer` means, not merely that this rung is empty.** Empty is
        // the right answer here only because MariaDB's client is opportunistic —
        // measured on 10.11.14: no `--ssl` flag against a server with
        // `have_ssl = YES` gives `Ssl_cipher = TLS_AES_256_GCM_SHA384`. So the
        // property is "tries TLS, requires nothing, verifies nothing": it must
        // not *demand* TLS (`--ssl`), must not *refuse* it (`--skip-ssl`), and
        // must not verify (`--ssl-verify-server-cert`). Asserting `is_empty()`
        // alone said nothing about any of those, so an arm that started sending
        // `--skip-ssl` here would have been caught by nothing.
        let prefer = args(SslMode::Prefer);
        assert!(
            !prefer.iter().any(|a| a == "--skip-ssl"),
            "Prefer must not refuse TLS: {prefer:?}"
        );
        assert!(
            !prefer.iter().any(|a| a == "--ssl"),
            "Prefer must not require TLS — that is what Require is: {prefer:?}"
        );
        assert!(
            !prefer.iter().any(|a| a == "--ssl-verify-server-cert"),
            "Prefer verifies nothing: {prefer:?}"
        );
        // And it is strictly weaker than the rung above it.
        assert_ne!(prefer, args(SslMode::Require));
        assert_eq!(args(SslMode::Require), ["--ssl"]);
        assert_eq!(args(SslMode::VerifyCa), ["--ssl"]);
        assert_eq!(
            args(SslMode::VerifyFull),
            ["--ssl", "--ssl-verify-server-cert"]
        );
        // The CA and client identity are spelled the same on both clients.
        assert_eq!(
            mysql_cli_tls_args(
                &Tls {
                    mode: SslMode::VerifyFull,
                    ca_path: "/etc/ca.crt".into(),
                    ..Tls::default()
                },
                MysqlClient::MariaDb,
            ),
            ["--ssl", "--ssl-verify-server-cert", "--ssl-ca=/etc/ca.crt"]
        );
        assert_eq!(MysqlClient::of("mariadb"), MysqlClient::MariaDb);
        assert_eq!(MysqlClient::of("/usr/bin/mariadb"), MysqlClient::MariaDb);
        assert_eq!(
            MysqlClient::of(r"C:\Program Files\MariaDBin\mariadb.exe"),
            MysqlClient::MariaDb
        );
        assert_eq!(MysqlClient::of("mysql"), MysqlClient::Mysql);
    }

    #[test]
    fn the_mysql_client_gets_the_ca_and_client_identity() {
        let tls = Tls {
            mode: SslMode::VerifyFull,
            ca_path: "/etc/ca.crt".into(),
            client_cert_path: "/etc/c.crt".into(),
            client_key_path: "/etc/c.key".into(),
        };
        assert_eq!(
            mysql_cli_tls_args(&tls, MysqlClient::Mysql),
            [
                "--ssl-mode=VERIFY_IDENTITY",
                "--ssl-ca=/etc/ca.crt",
                "--ssl-cert=/etc/c.crt",
                "--ssl-key=/etc/c.key",
            ]
        );
        // A non-verifying mode names no CA — the same rule `Tls::ca_file`
        // already encodes, asserted through this composition rather than again
        // on the accessor.
        let prefer = Tls {
            mode: SslMode::Prefer,
            ..tls.clone()
        };
        assert!(
            !mysql_cli_tls_args(&prefer, MysqlClient::Mysql)
                .iter()
                .any(|a| a.contains("ca"))
        );
        // Half a client pair is a failed handshake, not a weaker one.
        let half = Tls {
            client_key_path: String::new(),
            ..tls
        };
        assert_eq!(
            mysql_cli_tls_args(&half, MysqlClient::Mysql),
            ["--ssl-mode=VERIFY_IDENTITY", "--ssl-ca=/etc/ca.crt"]
        );
    }

    /// **A tunnelled endpoint and a verifying rung cannot both be honoured by a
    /// CLI**, and the builders were handed the rewritten `Connection` with its
    /// `tls` block untouched. `Db::connect` moves the address and carries a
    /// `hostname_override` in the same step, with a test saying why; neither
    /// `mysql` nor `psql` has that option, so the client was told to dial
    /// `127.0.0.1` and check the server's certificate against it — and refused
    /// its own server's certificate. Before the TLS arguments existed the client
    /// fell back to a non-verifying default and the session opened.
    #[test]
    fn a_tunnelled_connection_is_refused_only_at_a_verifying_rung() {
        let at = |mode| {
            tunnelled_verify_blocker(&Tls {
                mode,
                ..Tls::default()
            })
        };
        assert!(at(SslMode::VerifyFull).is_some());
        assert!(at(SslMode::VerifyCa).is_some());
        // Nothing below names a certificate, so the loopback address is the
        // address and there is nothing to compare it against.
        assert!(at(SslMode::Require).is_none());
        assert!(at(SslMode::Prefer).is_none());
        assert!(at(SslMode::Disable).is_none());
    }

    #[test]
    fn only_a_windows_shaped_cert_path_blocks_a_wsl_client() {
        let with = |ca: &str| {
            wsl_tls_blocker(&Tls {
                mode: SslMode::VerifyFull,
                ca_path: ca.into(),
                ..Tls::default()
            })
        };
        assert!(with(r"C:\certs\ca.crt").is_some());
        assert!(with("C:/certs/ca.crt").is_some());
        assert!(with(r"\\server\share\ca.crt").is_some());
        assert!(with("/etc/ca.crt").is_none());
        assert!(with("/mnt/c/certs/ca.crt").is_none());
        assert!(with("").is_none(), "no file named is not a blocker");
        // Any of the three paths, not just the CA.
        assert!(
            wsl_tls_blocker(&Tls {
                mode: SslMode::VerifyFull,
                client_key_path: r"C:\c.key".into(),
                ..Tls::default()
            })
            .is_some()
        );
    }

    #[test]
    fn psql_gets_its_mode_in_the_environment_for_every_rung() {
        for mode in SslMode::ALL {
            let env = psql_cli_tls_env(&Tls {
                mode,
                ..Tls::default()
            });
            assert_eq!(env[0].0, "PGSSLMODE");
            assert!(
                !env[0].1.is_empty(),
                "{mode:?} would leave libpq on its `prefer` default"
            );
        }
        let strict = psql_cli_tls_env(&Tls {
            mode: SslMode::VerifyFull,
            ca_path: "/etc/ca.crt".into(),
            client_cert_path: "/etc/c.crt".into(),
            client_key_path: "/etc/c.key".into(),
        });
        assert_eq!(
            strict,
            [
                ("PGSSLMODE".to_string(), "verify-full".to_string()),
                ("PGSSLROOTCERT".to_string(), "/etc/ca.crt".to_string()),
                ("PGSSLCERT".to_string(), "/etc/c.crt".to_string()),
                ("PGSSLKEY".to_string(), "/etc/c.key".to_string()),
            ]
        );
    }

    fn sqlcmd(host: &str, db: Option<&str>, mode: SslMode) -> Result<Vec<String>, Refusal> {
        sqlcmd_args(
            host,
            1433,
            crate::connection::AuthMode::Password,
            "sa",
            db,
            &Tls {
                mode,
                ..Tls::default()
            },
        )
    }

    /// **`sqlcmd` signs in the way the connection does**: a SQL login as
    /// `-U`, Windows sign-in as `-E` — a trusted connection, the process's own
    /// identity, in both clients — with no user sent. An Entra connection is
    /// refused: `sqlcmd` cannot be handed the Azure CLI's token, and falling
    /// back to a SQL login would be a different sign-in than the one saved.
    #[test]
    fn sqlcmd_signs_in_the_way_the_connection_does() {
        use crate::connection::AuthMode;
        let args = |auth| sqlcmd_args("db.example", 1433, auth, "sa", None, &Tls::default());
        assert!(
            args(AuthMode::Password)
                .unwrap()
                .contains(&"-Usa".to_string())
        );
        let windows = args(AuthMode::Windows).unwrap();
        assert!(windows.contains(&"-E".to_string()), "{windows:?}");
        assert!(!windows.iter().any(|a| a.starts_with("-U")), "{windows:?}");
        assert!(args(AuthMode::AzureCli).is_err());
    }

    /// `sqlcmd`'s argv: the server forced onto TCP, every value **attached**
    /// to its flag, so no value is ever read as an option — measured against
    /// ODBC `sqlcmd` 18, a database named `-x` or `x;Server=192.0.2.1` opens
    /// that database, verbatim, when passed this way — and `-I`, since the
    /// client's `QUOTED_IDENTIFIER` is off by default and Schemaic's sessions
    /// have it on.
    #[test]
    fn sqlcmd_takes_every_value_attached_to_its_flag() {
        assert_eq!(
            sqlcmd("db.example", Some("-x;Server=192.0.2.1"), SslMode::Require).unwrap(),
            [
                "-Stcp:db.example,1433",
                "-Usa",
                "-d-x;Server=192.0.2.1",
                "-I",
                "-Nm",
                "-C"
            ]
        );
        // No database: the login's default one, as the other clients do.
        let args = sqlcmd("h", None, SslMode::Require).unwrap();
        assert!(!args.iter().any(|a| a.starts_with("-d")), "{args:?}");
        // An IPv6 literal is bracketed, as the drivers write it.
        assert_eq!(
            sqlcmd("::1", None, SslMode::Require).unwrap()[0],
            "-Stcp:[::1],1433"
        );
    }

    /// The host is the one value that is parsed — `host,port`, `host\instance`
    /// — so what would re-shape it is refused rather than guessed at.
    #[test]
    fn a_host_sqlcmd_would_reparse_is_refused() {
        for host in ["h\\a\\b", "np:h\\SQLEXPRESS", "", "np:h", "h;x", "h,x"] {
            assert!(sqlcmd(host, None, SslMode::Require).is_err(), "{host:?}");
        }
        // **Read as the connect reads it** (S5-L1-02): an inline port wins
        // and `tcp:` / `(local)` resolve, so what connects in a query tab
        // opens here — it was refused, and the connect asked the Browser for
        // an instance named `SQLEXPRESS,1500`.
        for (host, address) in [
            ("h,1500", "-Stcp:h,1500"),
            ("h\\SQLEXPRESS,1500", "-Stcp:h,1500"),
            ("tcp:h\\SQLEXPRESS", "-Stcp:h\\SQLEXPRESS"),
            ("(local)\\SQLEXPRESS", "-Stcp:localhost\\SQLEXPRESS"),
        ] {
            let a =
                sqlcmd(host, None, SslMode::Require).unwrap_or_else(|e| panic!("{host}: {e:?}"));
            assert_eq!(a[0], address, "{host:?}");
        }
    }

    /// **A named instance is passed on as `sqlcmd` spells one** —
    /// `tcp:server\INSTANCE`, no port — and `sqlcmd` asks SQL Server Browser
    /// for the port, as the app's own connect does (measured: ODBC `sqlcmd`
    /// 18 answers an instance with no Browser behind it *Error Locating
    /// Server/Instance Specified*, not a syntax error).
    #[test]
    fn a_named_instance_reaches_sqlcmd_without_a_port() {
        let a = sqlcmd("db.corp\\SQLEXPRESS", None, SslMode::Require).unwrap();
        assert_eq!(a[0], "-Stcp:db.corp\\SQLEXPRESS");
    }

    /// Each rung lands on the flags that make `sqlcmd` behave as Schemaic's
    /// own connection does, or on a refusal where it cannot: measured, `-No`
    /// leaves the session unencrypted, `-Nm -C` encrypts it without judging
    /// the certificate, and `-Nm` alone verifies it (a self-signed one fails).
    #[test]
    fn each_tls_rung_lands_on_sqlcmds_flags() {
        let tail = |mode| {
            let a = sqlcmd("h", None, mode).unwrap();
            a[a.iter().position(|x| x == "-I").unwrap() + 1..].to_vec()
        };
        assert_eq!(tail(SslMode::Disable), ["-No"]);
        assert_eq!(tail(SslMode::Prefer), ["-Nm", "-C"]);
        assert_eq!(tail(SslMode::Require), ["-Nm", "-C"]);
        assert_eq!(tail(SslMode::VerifyCa), ["-Nm"]);
        assert_eq!(tail(SslMode::VerifyFull), ["-Nm"]);
        // A private CA is a file sqlcmd has no option for: `-J` pins the
        // server's own certificate, which is a different trust.
        let private = sqlcmd_args(
            "h",
            1433,
            crate::connection::AuthMode::Password,
            "sa",
            None,
            &Tls {
                mode: SslMode::VerifyFull,
                ca_path: "/etc/ca.crt".into(),
                ..Tls::default()
            },
        );
        assert!(private.is_err());
        // A client certificate likewise: SQL Server signs no one in by one,
        // and a session that silently leaves it out is not the one asked for.
        let client = sqlcmd_args(
            "h",
            1433,
            crate::connection::AuthMode::Password,
            "sa",
            None,
            &Tls {
                mode: SslMode::Require,
                client_cert_path: "/etc/c.crt".into(),
                client_key_path: "/etc/c.key".into(),
                ..Tls::default()
            },
        );
        assert!(client.is_err());
    }

    /// **Each refusal is the one its function answers**, so the interface,
    /// which words a [`Refusal`] by its variant, says the right sentence —
    /// and every variant is reached by something.
    #[test]
    fn each_launcher_refuses_with_its_own_reason() {
        use crate::connection::{AuthMode, SslMode, Tls};
        let sqlcmd = |host: &str, auth, tls: &Tls| {
            sqlcmd_args(host, 1433, auth, "sa", None, tls).unwrap_err()
        };
        let mode = |mode| Tls {
            mode,
            ..Tls::default()
        };
        let reached = [
            direct_spawn_verdict(std::path::Path::new("mysql.cmd")).unwrap_err(),
            psql_target("a=b").unwrap_err(),
            psql_target("postgres://x").unwrap_err(),
            sqlite_target("-x.db").unwrap_err(),
            sqlcmd("h", AuthMode::AzureCli, &mode(SslMode::Require)),
            sqlcmd("a,b", AuthMode::Password, &mode(SslMode::Require)),
            sqlcmd(
                "h",
                AuthMode::Password,
                &Tls {
                    mode: SslMode::VerifyFull,
                    ca_path: "/ca.pem".into(),
                    ..Tls::default()
                },
            ),
            sqlcmd(
                "h",
                AuthMode::Password,
                &Tls {
                    mode: SslMode::Require,
                    client_cert_path: "/c.pem".into(),
                    client_key_path: "/k.pem".into(),
                    ..Tls::default()
                },
            ),
            wsl_tls_blocker(&Tls {
                ca_path: "C:/ca.pem".into(),
                ..Tls::default()
            })
            .unwrap(),
            tunnelled_verify_blocker(&mode(SslMode::VerifyFull)).unwrap(),
            docker_inspect_args("not an id").unwrap_err(),
            backup_file_target("a\nb").unwrap_err(),
            role_arg(Some("-x")).unwrap_err(),
            mysql_source_path("a;b.sql", false).unwrap_err(),
            AuthMode::AwsIam.external_client_refusal().unwrap(),
        ];
        assert_eq!(reached, Refusal::ALL);
    }
}
