//! A **Microsoft Entra** token for SQL Server, from the Azure CLI — the
//! process half of `AuthMode::AzureCli`; the pure half, and why it is shaped
//! this way, is [`schemaic_core::entra`].
//!
//! **Cached in memory until shortly before it expires**, because every `Db`
//! operation opens its own connection and the CLI takes a second or two to
//! answer — a token per connection would put that in front of every query.
//! One cache for the process: the token is the CLI's signed-in user's, for
//! one resource, whichever connection asks. Never written anywhere; the CLI
//! keeps the refresh.
//!
//! **And only while the CLI is still signed in as the same identity.** The
//! token names whoever was signed in when it was minted; after `az login` as
//! someone else, or `az logout`, the cache went on handing out the previous
//! identity's token — which the server accepts, being valid — for up to an
//! hour, and the MCP subprocess, with its own cache, could run as the other
//! one. So the cache is keyed on the CLI's profile file
//! ([`entra::azure_cli_profile`]), which every one of those commands
//! rewrites: its modification time and length, read as metadata, never its
//! contents. A profile that cannot be read keys nothing, and then a token is
//! reused only briefly ([`UNKEYED_SECS`]).

use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use schemaic_core::entra::{self, AzureToken};

use crate::DbError;

/// Which Azure CLI sign-in a token came from: its profile file's
/// modification time and length, or `None` where it cannot be read.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct CliIdentity {
    modified_ns: u128,
    len: u64,
}

/// A token in the cache, with what decides whether it may be handed out.
struct Cached {
    token: AzureToken,
    /// The epoch second it stops being handed out ([`usable_until`]).
    until: i64,
    /// When it was fetched, for a token with no identity to key it.
    fetched_at: i64,
    /// The CLI sign-in it came from.
    identity: Option<CliIdentity>,
}

static CACHE: Mutex<Option<Cached>> = Mutex::new(None);

/// How long a token is reused when the CLI's profile could not be read, so
/// there is nothing to notice a changed sign-in by — long enough to cover a
/// burst of per-operation connects, short enough that a switched identity is
/// picked up within a minute.
const UNKEYED_SECS: i64 = 60;

/// How long before its expiry a token is replaced, so one handed to a
/// connection is not already dead by the time the server reads it.
const MARGIN_SECS: i64 = 300;
/// How long to keep a token whose expiry the CLI did not say.
const UNDATED_SECS: i64 = 600;
/// How long the CLI may take. Its first run after an install compiles its
/// modules and can take tens of seconds.
const CLI_TIMEOUT: Duration = Duration::from_secs(60);

/// The epoch second a token fetched at `now` stops being handed out.
fn usable_until(t: &AzureToken, now: i64) -> i64 {
    match t.expires_at {
        Some(e) => e - MARGIN_SECS,
        None => now + UNDATED_SECS,
    }
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// May `cached` be handed out at `now`, with the CLI signed in as
/// `identity`? Not past its expiry margin, not under another sign-in than
/// the one it came from, and — with no identity to compare — not past
/// [`UNKEYED_SECS`].
fn reusable(cached: &Cached, now: i64, identity: Option<CliIdentity>) -> bool {
    now < cached.until
        && cached.identity == identity
        && (identity.is_some() || now < cached.fetched_at + UNKEYED_SECS)
}

/// The Azure CLI sign-in as it stands: its profile's metadata.
fn cli_identity() -> Option<CliIdentity> {
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" });
    let config = std::env::var_os("AZURE_CONFIG_DIR");
    let profile = entra::azure_cli_profile(config.as_deref(), home.as_deref())?;
    let meta = std::fs::metadata(profile).ok()?;
    let modified_ns = meta
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_nanos();
    Some(CliIdentity {
        modified_ns,
        len: meta.len(),
    })
}

/// An access token for Azure SQL, from the cache or the CLI.
pub(crate) async fn sql_token() -> Result<String, DbError> {
    let at = now();
    let identity = cli_identity();
    if let Ok(cache) = CACHE.lock()
        && let Some(cached) = cache.as_ref()
        && reusable(cached, at, identity)
    {
        return Ok(cached.token.token.clone());
    }
    let fresh = fetch().await?;
    let until = usable_until(&fresh, at);
    let token = fresh.token.clone();
    // The identity as it stands *after* the fetch, in case asking the CLI
    // touched its own profile.
    let identity = cli_identity();
    if let Ok(mut cache) = CACHE.lock() {
        *cache = Some(Cached {
            token: fresh,
            until,
            fetched_at: at,
            identity,
        });
    }
    Ok(token)
}

/// Forget the cached token — after the server refused one, so the next
/// connection asks the CLI again rather than handing over the same.
pub(crate) fn forget() {
    if let Ok(mut cache) = CACHE.lock() {
        *cache = None;
    }
}

async fn fetch() -> Result<AzureToken, DbError> {
    const NOT_FOUND: &str = "The Azure CLI (az) was not found. Install it and run `az login`, \
        or sign in with a SQL login.";
    let path = std::env::var_os("PATH");
    let program_files: Vec<std::path::PathBuf> = ["ProgramFiles", "ProgramFiles(x86)"]
        .iter()
        .filter_map(std::env::var_os)
        .map(std::path::PathBuf::from)
        .collect();
    let pf: Vec<&std::path::Path> = program_files.iter().map(|p| p.as_path()).collect();
    let az = entra::azure_cli_candidates(path.as_deref(), &pf, cfg!(windows))
        .into_iter()
        .find(|p| p.is_file())
        .ok_or_else(|| DbError::Connect(NOT_FOUND.to_string()))?;
    let (program, args) = entra::azure_cli_token_argv(&az, |p| p.is_file())
        .map_err(|m| DbError::Connect(m.to_string()))?;
    let mut cmd = tokio::process::Command::new(program);
    cmd.args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    // A GUI app's child gets a console window of its own otherwise — one
    // flashing up per token.
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let out = tokio::time::timeout(CLI_TIMEOUT, cmd.output())
        .await
        .map_err(|_| {
            DbError::Connect(format!(
                "The Azure CLI did not answer within {} seconds.",
                CLI_TIMEOUT.as_secs()
            ))
        })?
        .map_err(|e| DbError::Connect(format!("The Azure CLI could not be started: {e}")))?;
    if !out.status.success() {
        return Err(DbError::Connect(entra::failure_text(
            &String::from_utf8_lossy(&out.stderr),
        )));
    }
    entra::parse_token(&String::from_utf8_lossy(&out.stdout)).map_err(DbError::Connect)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **A token is handed out until five minutes before it expires**, so a
    /// connection is never given one that dies on the way; one whose expiry
    /// the CLI did not print is kept ten minutes and asked for again.
    #[test]
    fn a_token_is_replaced_before_it_expires() {
        let dated = AzureToken {
            token: "t".into(),
            expires_at: Some(10_000),
        };
        assert_eq!(usable_until(&dated, 1_000), 10_000 - 300);
        let undated = AzureToken {
            token: "t".into(),
            expires_at: None,
        };
        assert_eq!(usable_until(&undated, 1_000), 1_600);
    }

    /// **A token is handed out only under the CLI sign-in it came from.**
    /// After `az login` as someone else, or `az logout`, the profile changes
    /// and the cached token — the previous identity's, which the server would
    /// still accept — is not handed out again; with no profile to read, a
    /// token is reused only for a minute.
    #[test]
    fn a_token_is_not_handed_out_under_another_cli_sign_in() {
        let a = Some(CliIdentity {
            modified_ns: 1,
            len: 100,
        });
        let b = Some(CliIdentity {
            modified_ns: 2,
            len: 100,
        });
        let cached = |identity| Cached {
            token: AzureToken {
                token: "t".into(),
                expires_at: Some(10_000),
            },
            until: 9_700,
            fetched_at: 1_000,
            identity,
        };
        assert!(reusable(&cached(a), 5_000, a));
        assert!(!reusable(&cached(a), 5_000, b), "az login as someone else");
        assert!(!reusable(&cached(a), 5_000, None), "az logout");
        assert!(
            !reusable(&cached(None), 5_000, a),
            "a profile that appeared"
        );
        assert!(!reusable(&cached(a), 9_700, a), "past the expiry margin");
        // Unkeyed: a minute, not the token's hour.
        assert!(reusable(&cached(None), 1_000 + UNKEYED_SECS - 1, None));
        assert!(!reusable(&cached(None), 1_000 + UNKEYED_SECS, None));
    }
}
