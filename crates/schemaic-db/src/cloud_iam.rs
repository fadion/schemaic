//! A **cloud IAM token** for PostgreSQL or the MySQL family, from the AWS or
//! the Google Cloud CLI — the process half of `AuthMode::AwsIam` and
//! `AuthMode::GcpIam`; the pure half, and why it is shaped this way, is
//! [`schemaic_core::cloud_iam`]. `entra.rs` is the same thing for SQL Server.
//!
//! **Cached in memory**, because every `Db` operation opens its own connection
//! and a CLI takes a second or more to answer. An AWS token is for one host,
//! port and user, so the cache holds one per target; gcloud's is the
//! account's, one for all. A token is handed out for [`Cloud::reuse_secs`]
//! after it was fetched, and **only while the CLI holds the same sign-in** —
//! keyed on its own files' metadata ([`cloud_iam::identity_files`]), so a
//! switched profile or account is picked up at the next connect. Never written
//! anywhere; the CLI keeps the long-lived credentials.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use schemaic_core::cloud_iam::{self, Cloud};
use schemaic_core::model::SignInFailure;

use crate::DbError;

/// What a token was minted for: the cloud, and — for AWS, whose tokens are
/// signed per target — the host, port and user.
#[derive(Clone, PartialEq, Eq, Debug)]
struct Key {
    cloud: Cloud,
    target: Option<(String, u16, String)>,
}

/// The CLI's sign-in as its files stand: each one's modification time and
/// length, `None` for one that is not there.
type Identity = Vec<Option<(u128, u64)>>;

struct Cached {
    key: Key,
    token: String,
    fetched_at: u64,
    identity: Identity,
}

static CACHE: Mutex<Vec<Cached>> = Mutex::new(Vec::new());

/// How long a CLI may take — the Google Cloud CLI's first run after an
/// update can take tens of seconds.
const CLI_TIMEOUT: Duration = Duration::from_secs(60);

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// May `cached` be handed out for `key` at `now`, with the CLI's sign-in at
/// `identity`?
fn reusable(cached: &Cached, key: &Key, now: u64, identity: &Identity) -> bool {
    cached.key == *key
        && now < cached.fetched_at + cached.key.cloud.reuse_secs()
        && cached.identity == *identity
}

fn key_for(cloud: Cloud, host: &str, port: u16, user: &str) -> Key {
    Key {
        cloud,
        target: match cloud {
            Cloud::Aws => Some((host.to_ascii_lowercase(), port, user.to_string())),
            Cloud::Gcloud => None,
        },
    }
}

fn identity(cloud: Cloud) -> Identity {
    let home =
        std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from);
    let app_data = std::env::var_os("APPDATA").map(PathBuf::from);
    cloud_iam::identity_files(
        cloud,
        home.as_deref(),
        app_data.as_deref(),
        |k| std::env::var_os(k).map(PathBuf::from),
        |p| std::fs::read_to_string(p).ok(),
        cfg!(windows),
    )
    .iter()
    .map(|f| {
        let meta = std::fs::metadata(f).ok()?;
        let modified = meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
        Some((modified.as_nanos(), meta.len()))
    })
    .collect()
}

/// A token for `user` at `host:port`, from the cache or the CLI.
pub(crate) async fn token(
    cloud: Cloud,
    host: &str,
    port: u16,
    user: &str,
) -> Result<String, DbError> {
    let key = key_for(cloud, host, port, user);
    let at = now();
    let ident = identity(cloud);
    if let Ok(cache) = CACHE.lock()
        && let Some(hit) = cache.iter().find(|c| reusable(c, &key, at, &ident))
    {
        return Ok(hit.token.clone());
    }
    let token = fetch(cloud, host, port, user).await?;
    // The sign-in as it stands *after* the fetch, in case asking touched it
    // (an SSO refresh rewrites the AWS CLI's cache, gcloud its token store).
    let ident = identity(cloud);
    if let Ok(mut cache) = CACHE.lock() {
        cache.retain(|c| c.key != key);
        cache.push(Cached {
            key,
            token: token.clone(),
            fetched_at: at,
            identity: ident,
        });
    }
    Ok(token)
}

/// `failure` as the connect's error.
fn sign_in_failed(failure: SignInFailure) -> DbError {
    DbError::ConnectFailed(crate::ConnectFailure::SignIn(failure))
}

async fn fetch(cloud: Cloud, host: &str, port: u16, user: &str) -> Result<String, DbError> {
    let args = cloud_iam::token_args(cloud, host, port, user).map_err(sign_in_failed)?;
    let path = std::env::var_os("PATH");
    let program_files: Vec<PathBuf> = ["ProgramFiles", "ProgramFiles(x86)"]
        .iter()
        .filter_map(std::env::var_os)
        .map(PathBuf::from)
        .collect();
    let pf: Vec<&std::path::Path> = program_files.iter().map(|p| p.as_path()).collect();
    let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    let cli =
        cloud_iam::cli_candidates(cloud, path.as_deref(), &pf, local.as_deref(), cfg!(windows))
            .into_iter()
            .find(|p| p.is_file())
            .ok_or_else(|| sign_in_failed(SignInFailure::NotFound(cloud.into())))?;
    let (program, argv) =
        cloud_iam::token_argv(cloud, &cli, args, |p| p.is_file()).map_err(sign_in_failed)?;
    let mut cmd = tokio::process::Command::new(program);
    cmd.args(argv)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    // A GUI app's child gets a console window of its own otherwise.
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let out = tokio::time::timeout(CLI_TIMEOUT, cmd.output())
        .await
        .map_err(|_| {
            sign_in_failed(SignInFailure::NoAnswer {
                cli: cloud.into(),
                secs: CLI_TIMEOUT.as_secs(),
            })
        })?
        .map_err(|e| {
            sign_in_failed(SignInFailure::NotStarted {
                cli: cloud.into(),
                error: e.to_string(),
            })
        })?;
    if !out.status.success() {
        return Err(sign_in_failed(cloud_iam::failure(
            cloud,
            &String::from_utf8_lossy(&out.stderr),
        )));
    }
    cloud_iam::parse_token(&String::from_utf8_lossy(&out.stdout)).map_err(sign_in_failed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cached(key: Key, fetched_at: u64, identity: Identity) -> Cached {
        Cached {
            key,
            token: "t".into(),
            fetched_at,
            identity,
        }
    }

    /// **An AWS token is its target's**: signed for one host, port and user,
    /// it is not handed to another — where gcloud's, the account's, serves
    /// every connection.
    #[test]
    fn an_aws_token_is_handed_out_only_to_the_target_it_was_minted_for() {
        let id: Identity = vec![Some((1, 1))];
        let a = key_for(Cloud::Aws, "DB.example", 5432, "app");
        let c = cached(a.clone(), 1_000, id.clone());
        assert!(reusable(
            &c,
            &key_for(Cloud::Aws, "db.example", 5432, "app"),
            1_001,
            &id
        ));
        assert!(!reusable(
            &c,
            &key_for(Cloud::Aws, "db.example", 5433, "app"),
            1_001,
            &id
        ));
        assert!(!reusable(
            &c,
            &key_for(Cloud::Aws, "db.example", 5432, "other"),
            1_001,
            &id
        ));
        let g = cached(key_for(Cloud::Gcloud, "a", 1, "x"), 1_000, id.clone());
        assert!(reusable(
            &g,
            &key_for(Cloud::Gcloud, "b", 2, "y"),
            1_001,
            &id
        ));
    }

    /// Replaced before it expires, and not under another CLI sign-in.
    #[test]
    fn a_token_expires_from_the_cache_and_does_not_outlive_its_sign_in() {
        let id: Identity = vec![Some((1, 1)), None];
        let key = key_for(Cloud::Aws, "h", 1, "u");
        let c = cached(key.clone(), 1_000, id.clone());
        let horizon = 1_000 + Cloud::Aws.reuse_secs();
        assert!(reusable(&c, &key, horizon - 1, &id));
        assert!(!reusable(&c, &key, horizon, &id));
        assert!(
            !reusable(&c, &key, 1_001, &vec![Some((2, 1)), None]),
            "aws configure"
        );
        assert!(
            !reusable(&c, &key, 1_001, &vec![None, None]),
            "credentials removed"
        );
    }
}
