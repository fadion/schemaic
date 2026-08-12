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
use std::sync::{Arc, Mutex};
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
    /// Filled by [`TunnelClient::check_server_key`] when it refuses a key.
    ///
    /// russh has **one** error for a rejected host key — `UnknownKey`, whose text
    /// is *"Unknown server key"* — so a changed key and a first-contact rejection
    /// are indistinguishable from the error alone. That text describes the
    /// opposite of a mismatch, and the natural remedy for it (clear the trust
    /// record) is exactly what hands the attacker's key a permanent welcome. So
    /// the verdict travels out of band and `open_tunnel` reports *that* instead.
    refusal: Arc<Mutex<Option<String>>>,
}

/// Serialises the known-hosts read-modify-write.
///
/// `check_server_key` loads the whole store, inserts one host and writes it all
/// back. Two tunnels opening at once — two connections restored at startup, or
/// **Test** pressed while another connects — would each load the same map and
/// each write their own, and the later write would drop the earlier host's
/// fingerprint. That host is then "unknown" on its next connection and silently
/// re-trusted, which is the same downgrade an unreadable store causes.
static STORE_LOCK: Mutex<()> = Mutex::new(());

/// The trust-on-first-use verdict for an offered server key (review H10). Pure
/// so the security decision can be exhaustively unit-tested; the I/O wrapper in
/// [`TunnelClient::check_server_key`] loads the store, reports, and persists.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum HostKeyVerdict {
    /// Known host, key still matches → accept without touching the store.
    Accept,
    /// Known host, key changed → refuse (the MITM signal).
    Mismatch,
    /// The store exists and could not be read → refuse. **Not** the same as an
    /// empty store: this host may well be recorded in the bytes we can't parse.
    StoreUnreadable,
    /// Unknown host → record the key, then accept (first-use trust).
    RecordAndAccept,
}

/// Decide how to treat `fingerprint` offered by `host_port`, given the current
/// known-hosts `store`. No I/O — the caller applies the verdict.
///
/// `store` is a `Result` on purpose. A general config loader answers "unreadable"
/// with `Default::default()`, which for a trust store is an *empty* one — and an
/// empty trust store trusts everything on first use. That is the right policy for
/// a window size and the wrong one here, so the failure has to survive as far as
/// this decision instead of being flattened into a value on the way.
fn known_host_decision(
    store: Result<&HashMap<String, String>, &str>,
    host_port: &str,
    fingerprint: &str,
) -> HostKeyVerdict {
    let Ok(store) = store else {
        return HostKeyVerdict::StoreUnreadable;
    };
    match store.get(host_port) {
        Some(known) if known == fingerprint => HostKeyVerdict::Accept,
        Some(_) => HostKeyVerdict::Mismatch,
        None => HostKeyVerdict::RecordAndAccept,
    }
}

/// What to tell the user about a refusal, or `None` when the key was accepted.
///
/// This is the security control's actual output. The refusal itself has always
/// worked; what didn't was saying so — the diagnosis went to `tracing::error!`,
/// and a released build has no console (`windows_subsystem = "windows"`) and no
/// log file, so the one message that matters most was written where nobody could
/// read it while the user saw "Unknown server key" and was invited to conclude
/// the host was merely new.
///
/// The order of the sentences is load-bearing and is asserted by a test: the
/// out-of-band verification comes *before* the file that re-trusts, because a
/// remedy read first is a remedy applied first.
fn refusal_message(
    verdict: HostKeyVerdict,
    host_port: &str,
    known: Option<&str>,
    offered: &str,
) -> Option<String> {
    match verdict {
        HostKeyVerdict::Accept | HostKeyVerdict::RecordAndAccept => None,
        HostKeyVerdict::Mismatch => Some(format!(
            "The SSH host key for {host_port} has CHANGED since Schemaic first trusted it.\n\n\
             Expected {}\nOffered  {offered}\n\n\
             This can mean the server was rebuilt or its key rotated — or that something is \
             intercepting the connection. Nothing was sent to it. Verify the new fingerprint out \
             of band (on the server's own console, or with whoever administers it) before \
             trusting it. Once you have, remove this host's entry from {KNOWN_HOSTS_FILE} in \
             Schemaic's config directory to record the new key.",
            known.unwrap_or("(no recorded key)")
        )),
        HostKeyVerdict::StoreUnreadable => Some(format!(
            "Schemaic could not read its record of trusted SSH host keys \
             ({KNOWN_HOSTS_FILE}), so it can't tell whether {host_port}'s key \
             ({offered}) is the one you trusted before.\n\n\
             The connection was refused rather than trusting the key on sight. Repair or delete \
             {KNOWN_HOSTS_FILE} in Schemaic's config directory — deleting it means every SSH host \
             is trusted afresh on its next connection, so do that only if you can verify the \
             fingerprints."
        )),
    }
}

impl Handler for TunnelClient {
    type Error = russh::Error;

    // Trust-on-first-use host-key verification (review H10): accept and record an
    // unknown host's key; accept a known host only if the key still matches;
    // refuse a changed key (the MITM signal). Fingerprints are SHA256.
    async fn check_server_key(&mut self, key: &ssh_key::PublicKey) -> Result<bool, Self::Error> {
        let fingerprint = key.fingerprint(ssh_key::HashAlg::Sha256).to_string();
        // Held across load → decide → insert → save, so a concurrent tunnel
        // can't read the same map and write back a version missing this host.
        // A poisoned lock means another thread panicked mid-update, which is
        // precisely when the store's contents are least worth trusting.
        let _guard = match STORE_LOCK.lock() {
            Ok(g) => g,
            Err(_) => {
                *self.refusal.lock().unwrap_or_else(|e| e.into_inner()) = refusal_message(
                    HostKeyVerdict::StoreUnreadable,
                    &self.host_port,
                    None,
                    &fingerprint,
                );
                return Ok(false);
            }
        };
        let loaded: Result<HashMap<String, String>, String> =
            schemaic_core::persist::load_json_strict(KNOWN_HOSTS_FILE);
        let verdict = known_host_decision(
            loaded.as_ref().map_err(String::as_str),
            &self.host_port,
            &fingerprint,
        );
        let known = loaded
            .as_ref()
            .ok()
            .and_then(|s| s.get(&self.host_port))
            .map(String::as_str);
        if let Some(msg) = refusal_message(verdict, &self.host_port, known, &fingerprint) {
            // Still logged for anyone running with a console; the connection
            // error is what the user actually sees.
            tracing::error!("SSH host-key refusal for {}: {msg}", self.host_port);
            *self.refusal.lock().unwrap_or_else(|e| e.into_inner()) = Some(msg);
            return Ok(false);
        }
        if verdict == HostKeyVerdict::RecordAndAccept {
            tracing::info!(
                "SSH host {} not seen before; trusting key {fingerprint} (TOFU)",
                self.host_port
            );
            let mut store = loaded.unwrap_or_default();
            store.insert(self.host_port.clone(), fingerprint);
            schemaic_core::persist::save_json(KNOWN_HOSTS_FILE, &store);
        }
        Ok(true)
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
    let refusal: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let handler = TunnelClient {
        host_port,
        refusal: refusal.clone(),
    };
    let mut session = client::connect(config, (ssh.host.as_str(), ssh.port), handler)
        .await
        .map_err(|e| {
            // A host-key refusal reaches here as russh's generic `UnknownKey`.
            // Our own verdict is the one worth showing.
            match refusal.lock().ok().and_then(|g| g.clone()) {
                Some(msg) => DbError::Connect(msg),
                None => DbError::Connect(format!("SSH connect failed: {e}")),
            }
        })?;

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
    use super::{HostKeyVerdict, known_host_decision, refusal_message};
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
            known_host_decision(Ok(&s), "db.example:22", "SHA256:abc"),
            HostKeyVerdict::RecordAndAccept
        );
    }

    #[test]
    fn known_host_matching_key_is_accepted() {
        let s = store(&[("db.example:22", "SHA256:abc")]);
        assert_eq!(
            known_host_decision(Ok(&s), "db.example:22", "SHA256:abc"),
            HostKeyVerdict::Accept
        );
    }

    #[test]
    fn known_host_changed_key_is_refused_as_mitm() {
        let s = store(&[("db.example:22", "SHA256:abc")]);
        assert_eq!(
            known_host_decision(Ok(&s), "db.example:22", "SHA256:DIFFERENT"),
            HostKeyVerdict::Mismatch
        );
    }

    #[test]
    fn decision_is_keyed_by_host_port() {
        // Same fingerprint recorded for a different host must not vouch for this one.
        let s = store(&[("other:22", "SHA256:abc")]);
        assert_eq!(
            known_host_decision(Ok(&s), "db.example:22", "SHA256:abc"),
            HostKeyVerdict::RecordAndAccept
        );
        // Same host, different port is a distinct entry.
        let s = store(&[("db.example:22", "SHA256:abc")]);
        assert_eq!(
            known_host_decision(Ok(&s), "db.example:2222", "SHA256:abc"),
            HostKeyVerdict::RecordAndAccept
        );
    }

    #[test]
    fn an_unreadable_store_is_not_an_empty_store() {
        // The whole point: "I have no record" and "I could not read my records"
        // must not mean the same thing. An empty store trusts on first use; an
        // unreadable one refuses, because a previously-trusted host would
        // otherwise be silently re-trusted with whatever key is offered.
        assert_eq!(
            known_host_decision(Err("bad json"), "db.example:22", "SHA256:abc"),
            HostKeyVerdict::StoreUnreadable
        );
        let empty = store(&[]);
        assert_eq!(
            known_host_decision(Ok(&empty), "db.example:22", "SHA256:abc"),
            HostKeyVerdict::RecordAndAccept
        );
    }

    #[test]
    fn a_mismatch_says_the_key_changed_and_carries_both_fingerprints() {
        let msg = refusal_message(
            HostKeyVerdict::Mismatch,
            "jump.example:22",
            Some("SHA256:old"),
            "SHA256:new",
        )
        .expect("a refusal must be explainable");
        assert!(msg.contains("CHANGED"), "{msg}");
        assert!(msg.contains("jump.example:22"), "{msg}");
        assert!(
            msg.contains("SHA256:old") && msg.contains("SHA256:new"),
            "{msg}"
        );
        // The remedy must not come before the verification that makes it safe.
        let verify = msg
            .find("erify")
            .expect("must ask for out-of-band checking");
        let remedy = msg
            .find(super::KNOWN_HOSTS_FILE)
            .expect("must say how to re-trust");
        assert!(
            verify < remedy,
            "the file is named before the warning: {msg}"
        );
    }

    #[test]
    fn an_unreadable_store_does_not_claim_the_key_changed() {
        // Naming the wrong cause is the defect being fixed, not a lesser version
        // of it: "the key changed" would send the user hunting an intruder, and
        // "unknown host" invites them to delete the record that protects them.
        let msg = refusal_message(
            HostKeyVerdict::StoreUnreadable,
            "jump.example:22",
            None,
            "SHA256:new",
        )
        .expect("a refusal must be explainable");
        assert!(!msg.contains("CHANGED"), "{msg}");
        assert!(msg.contains(super::KNOWN_HOSTS_FILE), "{msg}");
    }

    #[test]
    fn an_accepted_key_has_nothing_to_explain() {
        for v in [HostKeyVerdict::Accept, HostKeyVerdict::RecordAndAccept] {
            assert_eq!(
                refusal_message(v, "h:22", Some("SHA256:a"), "SHA256:a"),
                None
            );
        }
    }
}
