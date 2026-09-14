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
//! 1. **No shell.** The argv this module produces is executed directly. Nothing
//!    it emits is parsed by `cmd`, `sh` or PowerShell, so a byte that is syntax
//!    to a shell is inert. [`url_open_argv`]'s own test pins that, because the
//!    alternative — filtering every metacharacter of every shell — is how `&`
//!    would come to be refused inside a query string while `%` still expanded.
//! 2. **The string is validated where it stops being data**, not at the site
//!    that produced it. A caller upstream may have its own reasons to be
//!    permissive (the terminal's link tagger admits `&` because a query string
//!    needs it); the launcher does not inherit those reasons.

/// The bytes a URL may contain, as an allowlist.
///
/// The unreserved and reserved sets of RFC 3986 — nothing else. That excludes
/// every ASCII control byte, the space, and the whole of `" < > ^ | \` `` ` ``
/// `{ }`, which is the intersection worth naming: those are bytes `cmd` and
/// `sh` treat as syntax and RFC 3986 has no use for. The sub-delims it *does*
/// admit — `&`, `;`, `$`, `'`, `(`, `)` — are syntax to one shell or another
/// and are kept anyway, because a real query string needs them and
/// [`url_open_argv`] hands them to no shell. Non-ASCII is excluded too, and
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
pub fn psql_target(db: &str) -> Result<&str, &'static str> {
    const URI_PREFIXES: [&str; 2] = ["postgresql://", "postgres://"];
    const EQUALS: &str = "This database's name contains '=', which psql would read as a \
        connection string and follow to another server. Open it in a query tab instead.";
    const URI: &str = "This database's name begins like a connection URI, which psql would \
        follow to another server. Open it in a query tab instead.";
    if db.contains('=') {
        return Err(EQUALS);
    }
    // `get`, not `[..n]` — see `strip_scheme`. The name is read verbatim out of
    // `pg_database.datname`, is not the user's to vouch for, and this runs on
    // the UI thread from a click handler, where a panic takes the app rather
    // than reaching the panel the way every other refusal here does.
    if URI_PREFIXES
        .iter()
        .any(|p| db.get(..p.len()).is_some_and(|s| s.eq_ignore_ascii_case(p)))
    {
        return Err(URI);
    }
    Ok(db)
}

/// The `--ssl-*` argv a MySQL/MariaDB client needs to honour `tls`.
///
/// **Always non-empty, including for [`SslMode::Disable`][crate::connection::SslMode::Disable].**
/// Saying nothing is
/// not neutral: the client's own default is `--ssl-mode=PREFERRED`, which
/// accepts an unencrypted socket if the server offers one and verifies no
/// certificate either way — so an omitted flag downgrades a `verify-full`
/// connection while the app's own header, whose socket really is encrypted,
/// goes on reporting TLS.
pub fn mysql_cli_tls_args(tls: &crate::connection::Tls) -> Vec<String> {
    use crate::connection::SslMode;
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
pub fn wsl_tls_blocker(tls: &crate::connection::Tls) -> Option<&'static str> {
    const WHY: &str = "This connection's TLS certificate paths are Windows paths, which the WSL \
        client cannot open. Install a native client, or move the certificates inside WSL.";
    let paths = [&tls.ca_path, &tls.client_cert_path, &tls.client_key_path];
    paths.iter().any(|p| is_windows_path(p)).then_some(WHY)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connection::{SslMode, Tls};

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
        }
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
            mysql_cli_tls_args(&Tls {
                mode,
                ..Tls::default()
            })
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

    #[test]
    fn the_mysql_client_gets_the_ca_and_client_identity() {
        let tls = Tls {
            mode: SslMode::VerifyFull,
            ca_path: "/etc/ca.crt".into(),
            client_cert_path: "/etc/c.crt".into(),
            client_key_path: "/etc/c.key".into(),
        };
        assert_eq!(
            mysql_cli_tls_args(&tls),
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
        assert!(!mysql_cli_tls_args(&prefer).iter().any(|a| a.contains("ca")));
        // Half a client pair is a failed handshake, not a weaker one.
        let half = Tls {
            client_key_path: String::new(),
            ..tls
        };
        assert_eq!(
            mysql_cli_tls_args(&half),
            ["--ssl-mode=VERIFY_IDENTITY", "--ssl-ca=/etc/ca.crt"]
        );
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
}
