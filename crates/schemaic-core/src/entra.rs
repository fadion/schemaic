//! Signing in to SQL Server with a **Microsoft Entra** token from the Azure
//! CLI — the pure half of `connection::AuthMode::AzureCli`.
//!
//! The token is the Azure CLI's to mint and refresh: `az account
//! get-access-token --resource https://database.windows.net/` answers with
//! one for whoever is signed in to the CLI, and the TDS login hands it to the
//! server in place of a password. Nothing here stores one — `schemaic_db`'s
//! `entra` module asks for it when a connection needs it and keeps it only
//! in memory, until shortly before it expires.
//!
//! What lives here is what can be tested without a CLI: where to look for
//! `az` ([`azure_cli_candidates`]), the argv that runs it
//! ([`azure_cli_token_argv`]), reading its answer ([`parse_token`]) and
//! saying what went wrong in words a user can act on ([`failure_text`]), and
//! which file's change means the CLI's sign-in changed
//! ([`azure_cli_profile`]).

use std::path::{Path, PathBuf};

/// The resource a token for Azure SQL is minted for — its audience.
pub const AZURE_SQL_RESOURCE: &str = "https://database.windows.net/";

/// The arguments after the Azure CLI's own entry point.
const TOKEN_ARGS: [&str; 6] = [
    "account",
    "get-access-token",
    "--resource",
    AZURE_SQL_RESOURCE,
    "--output",
    "json",
];

/// Where `az` may be, most likely first: every absolute `PATH` directory in
/// order, then — on Windows — the Azure CLI installer's own folders, which
/// catch a CLI installed after the app started and so missing from the
/// `PATH` it inherited.
///
/// A relative `PATH` entry is skipped: `split_paths` yields an empty one for
/// the trailing `;` most Windows `PATH`s carry, and joining a name onto it is
/// a cwd-relative path (see `agent_cli::which_on_path`, where that shipped).
pub fn azure_cli_candidates(
    path: Option<&std::ffi::OsStr>,
    program_files: &[&Path],
    windows: bool,
) -> Vec<PathBuf> {
    let names: &[&str] = if windows {
        &["az.cmd", "az.bat", "az.exe"]
    } else {
        &["az"]
    };
    let mut out = Vec::new();
    if let Some(path) = path {
        for dir in std::env::split_paths(path) {
            if dir.as_os_str().is_empty() || dir.is_relative() {
                continue;
            }
            out.extend(names.iter().map(|n| dir.join(n)));
        }
    }
    if windows {
        for pf in program_files {
            out.push(
                pf.join("Microsoft SDKs")
                    .join("Azure")
                    .join("CLI2")
                    .join("wbin")
                    .join("az.cmd"),
            );
        }
    }
    out
}

/// The program and argv that ask the Azure CLI at `az` for an Azure SQL
/// token — **with no shell in between** (`launch`'s rule 1).
///
/// On Windows `az` is `az.cmd`, a batch file, and `CreateProcess` runs one
/// through `cmd.exe`. The installer's shim is two lines that run the Python
/// it ships — `"%~dp0\..\python.exe" -IBm azure.cli %*` — so this runs that
/// Python directly, found beside the shim or one folder up (a virtualenv's
/// `Scripts` holds both; the installer's `wbin` sits under the Python). A
/// shim with no Python where the installer puts one is refused, as
/// `launch::direct_spawn_verdict` refuses one, rather than handed to `cmd`.
///
/// Elsewhere `az` is run as found. It is a script there too — the same named
/// exception `xdg-open` is in `launch` — and every argument is a constant.
pub fn azure_cli_token_argv(
    az: &Path,
    exists: impl Fn(&Path) -> bool,
) -> Result<(PathBuf, Vec<String>), &'static str> {
    const SHIM: &str = "The Azure CLI found is a .cmd/.bat shim with no Python beside it, and \
        Schemaic does not run a shim through cmd.exe. Reinstall the Azure CLI with Microsoft's \
        installer, or sign in with a SQL login.";
    let args = || TOKEN_ARGS.iter().map(|a| a.to_string());
    let is_shim = az
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("cmd") || e.eq_ignore_ascii_case("bat"));
    if !is_shim {
        return Ok((az.to_path_buf(), args().collect()));
    }
    let dir = az.parent().ok_or(SHIM)?;
    let python = [
        dir.join("python.exe"),
        dir.parent()
            .map(|p| p.join("python.exe"))
            .unwrap_or_default(),
    ]
    .into_iter()
    .find(|p| !p.as_os_str().is_empty() && exists(p))
    .ok_or(SHIM)?;
    let mut argv = vec!["-IBm".to_string(), "azure.cli".to_string()];
    argv.extend(args());
    Ok((python, argv))
}

/// An access token and, when the CLI said, when it stops working.
#[derive(Clone, PartialEq, Eq)]
pub struct AzureToken {
    pub token: String,
    /// Seconds since the Unix epoch — the CLI's `expires_on`. `None` from a
    /// CLI too old to print it; the caller then keeps the token briefly.
    pub expires_at: Option<i64>,
}

/// Redacting: the token is a bearer credential, and a `Debug` that printed
/// it would put it in the log the Settings pane invites users to share — the
/// reason `Db`'s is hand-written.
impl std::fmt::Debug for AzureToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AzureToken")
            .field("token", &"<redacted>")
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

/// Read `az account get-access-token --output json`'s answer.
pub fn parse_token(stdout: &str) -> Result<AzureToken, String> {
    let v: serde_json::Value = serde_json::from_str(stdout.trim())
        .map_err(|_| "The Azure CLI's answer was not the token it should have printed.")?;
    let token = v
        .get("accessToken")
        .and_then(|t| t.as_str())
        .filter(|t| !t.trim().is_empty())
        .ok_or("The Azure CLI answered without an access token.")?
        .trim()
        .to_string();
    // A number on current CLIs; a string on some, which parses the same.
    let expires_at = v.get("expires_on").and_then(|e| {
        e.as_i64()
            .or_else(|| e.as_str().and_then(|s| s.trim().parse().ok()))
    });
    Ok(AzureToken { token, expires_at })
}

/// What went wrong asking the Azure CLI, in a sentence that says what to do.
/// `stderr` is the CLI's own; its `ERROR:` prefix is dropped and the first
/// line kept, since the rest is its help text.
pub fn failure_text(stderr: &str) -> String {
    let lower = stderr.to_ascii_lowercase();
    if lower.contains("az login") {
        return "The Azure CLI is not signed in. Run `az login`, then connect again.".to_string();
    }
    let first = stderr
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("it gave no reason");
    let first = first.strip_prefix("ERROR:").unwrap_or(first).trim();
    format!("The Azure CLI could not get a token: {first}")
}

/// Where the Azure CLI keeps its **profile** — which accounts are signed in
/// and which is the default — given `AZURE_CONFIG_DIR` and the home directory
/// (`USERPROFILE` on Windows, `HOME` elsewhere), or `None` with neither.
///
/// `az login` as someone else, `az logout` and `az account set` all rewrite
/// it, so its change is the signal that a cached token names the wrong
/// identity (`schemaic_db`'s `entra` keys its cache on it). Only its metadata
/// is ever read, never its contents.
pub fn azure_cli_profile(
    config_dir: Option<&std::ffi::OsStr>,
    home: Option<&std::ffi::OsStr>,
) -> Option<PathBuf> {
    const PROFILE: &str = "azureProfile.json";
    if let Some(dir) = config_dir.filter(|d| !d.is_empty()) {
        return Some(Path::new(dir).join(PROFILE));
    }
    let home = home.filter(|h| !h.is_empty())?;
    Some(Path::new(home).join(".azure").join(PROFILE))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The file whose change means the CLI's identity changed** — the
    /// Azure CLI's profile, in `AZURE_CONFIG_DIR` when that is set and in
    /// `.azure` under the home directory otherwise; `az login`, `az logout`
    /// and `az account set` all rewrite it.
    #[test]
    fn the_cli_profile_is_under_its_config_dir_or_the_home_directory() {
        use std::ffi::OsStr;
        let home = Some(OsStr::new("H"));
        assert_eq!(
            azure_cli_profile(Some(OsStr::new("C")), home),
            Some(Path::new("C").join("azureProfile.json"))
        );
        assert_eq!(
            azure_cli_profile(Some(OsStr::new("")), home),
            Some(Path::new("H").join(".azure").join("azureProfile.json"))
        );
        assert_eq!(
            azure_cli_profile(None, home),
            Some(Path::new("H").join(".azure").join("azureProfile.json"))
        );
        assert_eq!(azure_cli_profile(None, None), None);
        assert_eq!(azure_cli_profile(None, Some(OsStr::new(""))), None);
    }

    /// **`PATH` in order, relative entries skipped, then the installer's own
    /// folder on Windows** — which is what finds a CLI installed after the app
    /// started, whose inherited `PATH` predates it (seen on the machine this
    /// was written on).
    #[test]
    fn az_is_looked_for_on_path_then_where_the_installer_puts_it() {
        let path = std::env::join_paths([
            PathBuf::from(if cfg!(windows) {
                r"C:\tools"
            } else {
                "/usr/bin"
            }),
            PathBuf::from("relative"),
        ])
        .unwrap();
        let pf = PathBuf::from(r"C:\Program Files");
        let win = azure_cli_candidates(Some(&path), &[&pf], true);
        assert!(win[0].ends_with("az.cmd"), "{win:?}");
        assert!(!win.iter().any(|p| p.starts_with("relative")), "{win:?}");
        assert!(
            win.last().unwrap().ends_with(
                r"Microsoft SDKs/Azure/CLI2/wbin/az.cmd"
                    .replace('/', std::path::MAIN_SEPARATOR_STR)
            ),
            "{win:?}"
        );
        let unix = azure_cli_candidates(Some(&path), &[&pf], false);
        assert_eq!(unix.len(), 1, "{unix:?}");
        assert!(unix[0].ends_with("az"));
        assert!(azure_cli_candidates(None, &[], false).is_empty());
    }

    /// **The installer's `az.cmd` runs as its own Python**, never through
    /// `cmd.exe`; a shim without one is refused; a real executable or a Unix
    /// script runs as found. Every argument is a constant either way.
    #[test]
    fn the_token_request_runs_with_no_shell_in_between() {
        let wbin = Path::new("C:/CLI2/wbin/az.cmd");
        let (prog, argv) =
            azure_cli_token_argv(wbin, |p| p == Path::new("C:/CLI2/python.exe")).unwrap();
        assert_eq!(prog, Path::new("C:/CLI2/python.exe"));
        assert_eq!(argv[..2], ["-IBm", "azure.cli"]);
        assert_eq!(
            argv[2..],
            TOKEN_ARGS.map(str::to_string),
            "the request itself"
        );
        // A virtualenv's `Scripts` holds the Python beside the shim.
        let venv = Path::new("C:/venv/Scripts/az.bat");
        let (prog, _) =
            azure_cli_token_argv(venv, |p| p == Path::new("C:/venv/Scripts/python.exe")).unwrap();
        assert_eq!(prog, Path::new("C:/venv/Scripts/python.exe"));
        assert!(azure_cli_token_argv(wbin, |_| false).is_err());
        let (prog, argv) = azure_cli_token_argv(Path::new("/usr/bin/az"), |_| false).unwrap();
        assert_eq!(prog, Path::new("/usr/bin/az"));
        assert_eq!(argv, TOKEN_ARGS.map(str::to_string));
    }

    /// The CLI's answer, as measured from `az` 2.x on 2026-09-29: a JSON
    /// object with `accessToken` and an epoch `expires_on` — a number, or on
    /// some versions a string. Anything else is refused, not guessed at.
    #[test]
    fn a_token_is_read_from_the_clis_answer() {
        let t = parse_token(
            r#"{"accessToken":"eyJ0.abc","expiresOn":"2026-09-29 02:58:51.000000",
                "expires_on":1790643531,"tokenType":"Bearer"}"#,
        )
        .unwrap();
        assert_eq!(t.token, "eyJ0.abc");
        assert_eq!(t.expires_at, Some(1790643531));
        let s = parse_token(r#"{"accessToken":"x","expires_on":"1790643531"}"#).unwrap();
        assert_eq!(s.expires_at, Some(1790643531));
        let old = parse_token(r#"{"accessToken":"x","expiresOn":"2026-09-29 02:58:51"}"#).unwrap();
        assert_eq!(old.expires_at, None);
        assert!(parse_token("").is_err());
        assert!(parse_token(r#"{"accessToken":""}"#).is_err());
        assert!(parse_token("Please run 'az login'").is_err());
        assert!(!format!("{t:?}").contains("eyJ0"), "the token never prints");
    }

    /// A signed-out CLI says so and says what to run; anything else keeps the
    /// CLI's own first line, without its `ERROR:` prefix.
    #[test]
    fn a_failure_says_what_to_do() {
        let signed_out = "ERROR: Please run 'az login' to setup account.\n";
        assert!(failure_text(signed_out).contains("Run `az login`"));
        let other = "ERROR: (AADSTS700082) The refresh token has expired.\nmore help";
        assert_eq!(
            failure_text(other),
            "The Azure CLI could not get a token: (AADSTS700082) The refresh token has expired."
        );
        assert!(failure_text("").ends_with("it gave no reason"));
    }
}
