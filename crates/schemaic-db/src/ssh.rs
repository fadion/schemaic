//! SSH tunnelling for reaching a database server that isn't directly routable.
//!
//! [`open_tunnel`] connects+authenticates to the SSH server, binds a local
//! ephemeral port, and forwards each local connection to the target
//! `host:port` through an SSH `direct-tcpip` channel. The caller then points
//! the MySQL driver at `127.0.0.1:<local_port>`.
//!
//! Lifetime: the returned [`TunnelHandle`] owns the accept-loop task; dropping
//! it aborts that task, which drops the listener and frees the local port (so a
//! superseded or evicted tunnel doesn't leak a listener/port/task — review H9).
//! The SSH transport is configured with keepalives so a dead peer is detected
//! rather than silently reused.
//!
//! Security: the server host key is verified trust-on-first-use against a
//! Schemaic-managed store (`ssh_known_hosts.json`, `host:port` → SHA256
//! fingerprint). The first connection records the key; a later *mismatch* is
//! refused — that's the MITM signal (review H10).
//!
//! Authentication supports password, private-key file (optionally passphrase-
//! protected), and delegation to the running SSH agent (see [`authenticate`]).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use russh::client::{self, Handler};
use russh::keys::PrivateKeyWithHashAlg;
use tokio::io::copy_bidirectional;
use tokio::net::TcpListener;

use crate::DbError;
use schemaic_core::connection::{SshAuth, SshTunnel};

/// The persisted known-hosts store: `"host:port"` → server-key SHA256 fingerprint.
const KNOWN_HOSTS_FILE: &str = "ssh_known_hosts.json";

/// A live SSH tunnel. Dropping it aborts the accept loop, releasing the local
/// listener + port (and, once in-flight forwards finish, the SSH session).
pub struct TunnelHandle {
    port: u16,
    accept_task: tokio::task::AbortHandle,
}

impl TunnelHandle {
    /// The local `127.0.0.1` port the MySQL driver should connect to.
    pub fn port(&self) -> u16 {
        self.port
    }
}

impl Drop for TunnelHandle {
    fn drop(&mut self) {
        self.accept_task.abort();
    }
}

struct TunnelClient {
    /// `"host:port"` of the SSH server, for the known-hosts lookup.
    host_port: String,
}

/// The trust-on-first-use verdict for an offered server key (review H10). Pure
/// so the security decision can be exhaustively unit-tested; the I/O wrapper in
/// [`TunnelClient::check_server_key`] loads the store, logs, and persists.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum HostKeyVerdict {
    /// Known host, key still matches → accept without touching the store.
    Accept,
    /// Known host, key changed → refuse (the MITM signal).
    Refuse,
    /// Unknown host → record the key, then accept (first-use trust).
    RecordAndAccept,
}

/// Decide how to treat `fingerprint` offered by `host_port`, given the current
/// known-hosts `store`. No I/O — the caller applies the verdict.
fn known_host_decision(
    store: &HashMap<String, String>,
    host_port: &str,
    fingerprint: &str,
) -> HostKeyVerdict {
    match store.get(host_port) {
        Some(known) if known == fingerprint => HostKeyVerdict::Accept,
        Some(_) => HostKeyVerdict::Refuse,
        None => HostKeyVerdict::RecordAndAccept,
    }
}

impl Handler for TunnelClient {
    type Error = russh::Error;

    // Trust-on-first-use host-key verification (review H10): accept and record an
    // unknown host's key; accept a known host only if the key still matches;
    // refuse a changed key (the MITM signal). Fingerprints are SHA256.
    async fn check_server_key(&mut self, key: &ssh_key::PublicKey) -> Result<bool, Self::Error> {
        let fingerprint = key.fingerprint(ssh_key::HashAlg::Sha256).to_string();
        let mut store: HashMap<String, String> =
            schemaic_core::persist::load_json(KNOWN_HOSTS_FILE);
        match known_host_decision(&store, &self.host_port, &fingerprint) {
            HostKeyVerdict::Accept => Ok(true),
            HostKeyVerdict::Refuse => {
                let known = store.get(&self.host_port).map(String::as_str).unwrap_or("");
                tracing::error!(
                    "SSH host-key MISMATCH for {}: known {known}, offered {fingerprint} — refusing \
                     (possible MITM; remove it from {KNOWN_HOSTS_FILE} to re-trust)",
                    self.host_port
                );
                Ok(false)
            }
            HostKeyVerdict::RecordAndAccept => {
                tracing::info!(
                    "SSH host {} not seen before; trusting key {fingerprint} (TOFU)",
                    self.host_port
                );
                store.insert(self.host_port.clone(), fingerprint);
                schemaic_core::persist::save_json(KNOWN_HOSTS_FILE, &store);
                Ok(true)
            }
        }
    }
}

/// Authenticate the freshly connected SSH session per `ssh.auth`. Errors carry a
/// human-readable reason (surfaced by the Manage-Connections "Test" button).
async fn authenticate(
    session: &mut client::Handle<TunnelClient>,
    ssh: &SshTunnel,
) -> Result<(), DbError> {
    let ok = match ssh.auth {
        SshAuth::Password => session
            .authenticate_password(ssh.user.clone(), ssh.password.clone())
            .await
            .map_err(|e| DbError::Connect(format!("SSH auth error: {e}")))?
            .success(),
        SshAuth::KeyPair => authenticate_key(session, ssh).await?,
        SshAuth::Agent => authenticate_agent(session, &ssh.user).await?,
    };
    if ok {
        Ok(())
    } else {
        Err(DbError::Connect("SSH authentication failed".to_string()))
    }
}

/// Private-key-file auth: load the key (decrypting with the passphrase if the
/// key is encrypted), pick the best RSA hash the server advertises, and sign.
async fn authenticate_key(
    session: &mut client::Handle<TunnelClient>,
    ssh: &SshTunnel,
) -> Result<bool, DbError> {
    let passphrase = (!ssh.key_passphrase.is_empty()).then_some(ssh.key_passphrase.as_str());
    let key = russh::keys::load_secret_key(&ssh.key_path, passphrase)
        .map_err(|e| DbError::Connect(format!("SSH key load failed: {e}")))?;
    let hash_alg = session
        .best_supported_rsa_hash()
        .await
        .ok()
        .flatten()
        .flatten();
    let res = session
        .authenticate_publickey(
            ssh.user.clone(),
            PrivateKeyWithHashAlg::new(Arc::new(key), hash_alg),
        )
        .await
        .map_err(|e| DbError::Connect(format!("SSH key auth error: {e}")))?;
    Ok(res.success())
}

/// SSH-agent auth: ask the agent for its identities and try each public key,
/// delegating the signature to the agent. Transport-agnostic (Unix socket or
/// Windows named pipe / Pageant), so it's generic over the agent stream.
async fn agent_try<R>(
    session: &mut client::Handle<TunnelClient>,
    user: &str,
    mut agent: russh::keys::agent::client::AgentClient<R>,
) -> Result<bool, DbError>
where
    R: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send,
{
    use russh::keys::agent::AgentIdentity;
    let ids = agent
        .request_identities()
        .await
        .map_err(|e| DbError::Connect(format!("SSH agent error: {e}")))?;
    if ids.is_empty() {
        return Err(DbError::Connect(
            "SSH agent has no identities loaded (run `ssh-add`)".to_string(),
        ));
    }
    let hash_alg = session
        .best_supported_rsa_hash()
        .await
        .ok()
        .flatten()
        .flatten();
    for id in ids {
        if let AgentIdentity::PublicKey { key, .. } = id {
            let res = session
                .authenticate_publickey_with(user, key, hash_alg, &mut agent)
                .await
                .map_err(|e| DbError::Connect(format!("SSH agent auth error: {e}")))?;
            if res.success() {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// Connect to the platform SSH agent, then authenticate. Windows: the OpenSSH
/// named pipe first, then Pageant. Unix: `$SSH_AUTH_SOCK`.
#[cfg(windows)]
async fn authenticate_agent(
    session: &mut client::Handle<TunnelClient>,
    user: &str,
) -> Result<bool, DbError> {
    use russh::keys::agent::client::AgentClient;
    if let Ok(agent) = AgentClient::connect_named_pipe(r"\\.\pipe\openssh-ssh-agent").await {
        return agent_try(session, user, agent).await;
    }
    if let Ok(agent) = AgentClient::connect_pageant().await {
        return agent_try(session, user, agent).await;
    }
    Err(DbError::Connect(
        "no SSH agent found (start the OpenSSH Authentication Agent service, or run Pageant)"
            .to_string(),
    ))
}

#[cfg(unix)]
async fn authenticate_agent(
    session: &mut client::Handle<TunnelClient>,
    user: &str,
) -> Result<bool, DbError> {
    use russh::keys::agent::client::AgentClient;
    let agent = AgentClient::connect_env()
        .await
        .map_err(|e| DbError::Connect(format!("no SSH agent ($SSH_AUTH_SOCK): {e}")))?;
    agent_try(session, user, agent).await
}

/// Open an SSH tunnel to `target_host:target_port` and return a handle carrying
/// the local port a MySQL connection should use. The tunnel forwards connections
/// until the handle is dropped.
pub async fn open_tunnel(
    ssh: &SshTunnel,
    target_host: &str,
    target_port: u16,
) -> Result<TunnelHandle, DbError> {
    // Keepalives so a dropped SSH session is detected instead of the local port
    // being reused against a dead tunnel forever (review H9).
    let config = Arc::new(client::Config {
        keepalive_interval: Some(Duration::from_secs(30)),
        keepalive_max: 3,
        ..Default::default()
    });
    let host_port = format!("{}:{}", ssh.host, ssh.port);
    let handler = TunnelClient { host_port };
    let mut session = client::connect(config, (ssh.host.as_str(), ssh.port), handler)
        .await
        .map_err(|e| DbError::Connect(format!("SSH connect failed: {e}")))?;

    authenticate(&mut session, ssh).await?;

    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .map_err(|e| DbError::Connect(format!("local tunnel bind failed: {e}")))?;
    let local_port = listener
        .local_addr()
        .map_err(|e| DbError::Connect(e.to_string()))?
        .port();

    tracing::info!(
        "SSH tunnel up: 127.0.0.1:{local_port} → {target_host}:{target_port} via {}@{}:{}",
        ssh.user,
        ssh.host,
        ssh.port
    );

    let session = Arc::new(session);
    let target_host = target_host.to_string();
    let accept = tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                break;
            };
            let session = session.clone();
            let target_host = target_host.clone();
            tokio::spawn(async move {
                let channel = match session
                    .channel_open_direct_tcpip(target_host, target_port as u32, "127.0.0.1", 0)
                    .await
                {
                    Ok(c) => c,
                    Err(e) => {
                        tracing::warn!("tunnel channel open failed: {e}");
                        return;
                    }
                };
                let mut stream = channel.into_stream();
                let _ = copy_bidirectional(&mut socket, &mut stream).await;
            });
        }
    });

    Ok(TunnelHandle {
        port: local_port,
        accept_task: accept.abort_handle(),
    })
}

#[cfg(test)]
mod tests {
    use super::{HostKeyVerdict, known_host_decision};
    use std::collections::HashMap;

    fn store(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(h, f)| (h.to_string(), f.to_string()))
            .collect()
    }

    #[test]
    fn unknown_host_is_recorded_and_accepted() {
        let s = store(&[]);
        assert_eq!(
            known_host_decision(&s, "db.example:22", "SHA256:abc"),
            HostKeyVerdict::RecordAndAccept
        );
    }

    #[test]
    fn known_host_matching_key_is_accepted() {
        let s = store(&[("db.example:22", "SHA256:abc")]);
        assert_eq!(
            known_host_decision(&s, "db.example:22", "SHA256:abc"),
            HostKeyVerdict::Accept
        );
    }

    #[test]
    fn known_host_changed_key_is_refused_as_mitm() {
        let s = store(&[("db.example:22", "SHA256:abc")]);
        assert_eq!(
            known_host_decision(&s, "db.example:22", "SHA256:DIFFERENT"),
            HostKeyVerdict::Refuse
        );
    }

    #[test]
    fn decision_is_keyed_by_host_port() {
        // Same fingerprint recorded for a different host must not vouch for this one.
        let s = store(&[("other:22", "SHA256:abc")]);
        assert_eq!(
            known_host_decision(&s, "db.example:22", "SHA256:abc"),
            HostKeyVerdict::RecordAndAccept
        );
        // Same host, different port is a distinct entry.
        let s = store(&[("db.example:22", "SHA256:abc")]);
        assert_eq!(
            known_host_decision(&s, "db.example:2222", "SHA256:abc"),
            HostKeyVerdict::RecordAndAccept
        );
    }
}
