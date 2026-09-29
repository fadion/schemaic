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

use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use schemaic_core::entra::{self, AzureToken};

use crate::DbError;

/// The token and the epoch second it stops being handed out.
static CACHE: Mutex<Option<(AzureToken, i64)>> = Mutex::new(None);

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

/// An access token for Azure SQL, from the cache or the CLI.
pub(crate) async fn sql_token() -> Result<String, DbError> {
    let at = now();
    if let Ok(cache) = CACHE.lock()
        && let Some((t, until)) = cache.as_ref()
        && at < *until
    {
        return Ok(t.token.clone());
    }
    let fresh = fetch().await?;
    let until = usable_until(&fresh, at);
    let token = fresh.token.clone();
    if let Ok(mut cache) = CACHE.lock() {
        *cache = Some((fresh, until));
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
}
