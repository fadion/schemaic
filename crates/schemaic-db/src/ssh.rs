//! SSH tunnelling for reaching a database server that isn't directly routable.
//!
//! [`open_tunnel`] connects+authenticates to the SSH server, binds a local
//! ephemeral port, and forwards each local connection to the target
//! `host:port` through an SSH `direct-tcpip` channel. The caller then points
//! the MySQL driver at `127.0.0.1:<local_port>`.
//!
//! **Jump hosts** (`SshTunnel::jump`, OpenSSH's `ProxyJump`) are hops before
//! the SSH host: the first is dialled over TCP and each later one over a
//! `direct-tcpip` channel of the one before it, every hop authenticated and
//! its host key checked on its own `host:port`. The jump sessions live in the
//! accept loop beside the last one, whose transport they carry.
//!
//! **`~/.ssh/config` is read per tunnel** (`core::ssh_config::read_user_config`): the SSH host
//! and each jump host are looked up as `Host` aliases (`core::ssh_config`), and
//! the file fills what the connection leaves open, as `ssh <alias>` would. The
//! route is composed by `core::ssh_config::resolve_route`, which also refuses
//! one the file hides (a `ProxyCommand`, a `Match` block) and a password bound
//! for a jump host only the file names.
//!
//! Lifetime: the returned [`TunnelHandle`] owns the accept-loop task; dropping
//! it aborts that task, which drops the listener and frees the local port (so a
//! superseded or evicted tunnel doesn't leak a listener/port/task — review H9).
//! The SSH transport is configured with keepalives so a dead peer is detected
//! rather than silently reused.
//!
//! Security: the server host key is checked against the user's own
//! `~/.ssh/known_hosts` first — a listed key is accepted, a changed one refused
//! — and otherwise verified trust-on-first-use against a Schemaic-managed store
//! (`ssh_known_hosts.json`, `host:port` → SHA256 fingerprint). The first
//! connection records the key; a later *mismatch* is refused — that's the MITM
//! signal (review H10).
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
use schemaic_core::ssh_config;

/// The persisted known-hosts store: `"host:port"` → server-key SHA256 fingerprint.
const KNOWN_HOSTS_FILE: &str = "ssh_known_hosts.json";

/// How long [`open_tunnel`] waits for the SSH server to connect and finish the
/// key exchange. russh has no bound of its own here: its default
/// `inactivity_timeout` is `None`, `read_ssh_id` waits for a banner forever, and
/// the keepalives only start once a session exists. An endpoint that accepts TCP
/// and never speaks (a port-forward to a dead backend) held the connect spinner
/// indefinitely, and a host that drops packets held it for the OS connect timeout.
pub const TUNNEL_CONNECT_TIMEOUT: Duration = Duration::from_secs(20);

/// How long [`open_tunnel`] waits for authentication once the transport is up.
/// Longer than [`TUNNEL_CONNECT_TIMEOUT`] on purpose: an agent may ask a person
/// to approve the signature (1Password, Pageant with confirmation, a hardware
/// key's touch), and that wait is theirs, not the network's.
pub const TUNNEL_AUTH_TIMEOUT: Duration = Duration::from_secs(60);

/// `fut`, or the refusal naming the phase that did not answer within `limit`.
async fn within<T>(
    limit: Duration,
    phase: crate::SshPhase,
    host_port: &str,
    fut: impl std::future::Future<Output = Result<T, DbError>>,
) -> Result<T, DbError> {
    match tokio::time::timeout(limit, fut).await {
        Ok(r) => r,
        Err(_) => Err(ssh_failed(crate::SshFailure::NoAnswer {
            phase,
            host_port: host_port.to_string(),
            secs: limit.as_secs(),
        })),
    }
}

/// `failure` as the connect's error.
fn ssh_failed(failure: crate::SshFailure) -> DbError {
    DbError::ConnectFailed(crate::ConnectFailure::Ssh(failure))
}

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
    /// The keys the user's own `~/.ssh/known_hosts` lists for this server,
    /// with their line numbers — read once, before the connect, because they
    /// decide two things: which key types the client asks for
    /// ([`host_key_order`]) and then what the presented key is worth
    /// ([`openssh_verdict`]). Empty when the host is not listed or the file
    /// cannot be read.
    listed: Vec<(usize, ssh_key::PublicKey)>,
    /// Filled by [`TunnelClient::check_server_key`] when it refuses a key.
    ///
    /// russh has **one** error for a rejected host key — `UnknownKey`, whose text
    /// is *"Unknown server key"* — so a changed key and a first-contact rejection
    /// are indistinguishable from the error alone. That text describes the
    /// opposite of a mismatch, and the natural remedy for it (clear the trust
    /// record) is exactly what hands the attacker's key a permanent welcome. So
    /// the verdict travels out of band and `open_tunnel` reports *that* instead.
    refusal: Arc<Mutex<Option<crate::SshFailure>>>,
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
    /// Unknown host → record the key, then accept (first-use trust). Also a
    /// host `~/.ssh/known_hosts` vouches for that Schemaic's store does not
    /// yet record with this key: pinned here, so that a later key of another
    /// type — which `known_hosts` cannot judge — meets a [`Self::Mismatch`]
    /// rather than first-use trust.
    RecordAndAccept,
    /// `~/.ssh/known_hosts` records a different key of the offered type → refuse.
    OpenSshChanged,
    /// `~/.ssh/known_hosts` lists the host, but only under other key types
    /// than the one offered → refuse. The client asked for the listed types
    /// first ([`host_key_order`]), so a server answering with another one is
    /// either reconfigured or not the server `known_hosts` knows; OpenSSH asks
    /// the user here, and with no one to ask, trusting it is what not to do.
    OpenSshOtherType,
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

/// The whole host-key decision: what `~/.ssh/known_hosts` says, then
/// Schemaic's own store. One function so the two halves are tested *together*
/// — each was right alone while their composition let a key-type downgrade
/// through (release review S2-L5-01): a host `known_hosts` vouched for was
/// accepted without being pinned, so an attacker later offering a key type
/// `known_hosts` had no line for fell through to an empty store and was
/// trusted on first use, password and all.
fn host_key_decision(
    openssh: Option<OpenSshVerdict>,
    store: Result<&HashMap<String, String>, &str>,
    host_port: &str,
    fingerprint: &str,
) -> HostKeyVerdict {
    match openssh {
        Some(OpenSshVerdict::Changed { .. }) => HostKeyVerdict::OpenSshChanged,
        Some(OpenSshVerdict::OtherTypeOnly) => HostKeyVerdict::OpenSshOtherType,
        // Vouched for: accepted whatever the store holds — `known_hosts` is
        // the user's own word, and a rotation recorded there supersedes the
        // pin — and pinned when the store does not already say the same.
        Some(OpenSshVerdict::Known) => match store {
            Ok(s) if s.get(host_port).map(String::as_str) == Some(fingerprint) => {
                HostKeyVerdict::Accept
            }
            Ok(_) => HostKeyVerdict::RecordAndAccept,
            // Nothing to pin into; the next connect asks `known_hosts` again.
            Err(_) => HostKeyVerdict::Accept,
        },
        None => known_host_decision(store, host_port, fingerprint),
    }
}

/// What the user's own `~/.ssh/known_hosts` says about a host's key.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum OpenSshVerdict {
    /// Listed, with this key: accepted.
    Known,
    /// Listed with a different key of the same type, on `line`, and with
    /// this key on no line: refused.
    Changed { line: usize },
    /// Listed, but only under other key types: refused
    /// ([`HostKeyVerdict::OpenSshOtherType`]).
    OtherTypeOnly,
}

/// [`OpenSshVerdict`] for `offered`, given the keys `known_hosts` lists for the
/// host, or `None` when it lists none and Schemaic's own first-use trust
/// decides. Pure, so the decision has a test the filesystem does not take part
/// in.
///
/// Any listed key equal to the offered one is a match, wherever it sits — the
/// rule OpenSSH applies. russh's `check_known_hosts` stops at the first
/// same-type line that differs, so a host listed twice after a key rotation
/// (`UpdateHostKeys`, a second `ssh-keyscan >>`) was refused as an
/// interception OpenSSH connects to happily (release review S2-L1-02).
fn openssh_verdict(
    listed: &[(usize, ssh_key::PublicKey)],
    offered: &ssh_key::PublicKey,
) -> Option<OpenSshVerdict> {
    if listed.is_empty() {
        return None;
    }
    // Compared on the key alone: a comment is no part of what is trusted.
    if listed
        .iter()
        .any(|(_, k)| k.key_data() == offered.key_data())
    {
        return Some(OpenSshVerdict::Known);
    }
    match listed
        .iter()
        .find(|(_, k)| same_key_type(&k.algorithm(), &offered.algorithm()))
    {
        Some((line, _)) => Some(OpenSshVerdict::Changed { line: *line }),
        None => Some(OpenSshVerdict::OtherTypeOnly),
    }
}

/// Whether two host-key algorithms name one kind of key. The RSA signature
/// variants (`rsa-sha2-512`, `rsa-sha2-256`, `ssh-rsa`) all verify with the
/// one `ssh-rsa` key `known_hosts` records.
fn same_key_type(a: &ssh_key::Algorithm, b: &ssh_key::Algorithm) -> bool {
    let rsa = |x: &ssh_key::Algorithm| matches!(x, ssh_key::Algorithm::Rsa { .. });
    a == b || (rsa(a) && rsa(b))
}

/// The key types `known_hosts` lists, as the refusal names them:
/// `ssh-ed25519, ssh-rsa`, each once, in file order.
fn listed_types(listed: &[(usize, ssh_key::PublicKey)]) -> String {
    let mut types: Vec<String> = Vec::new();
    for (_, k) in listed {
        let t = k.algorithm().as_str().to_string();
        if !types.contains(&t) {
            types.push(t);
        }
    }
    types.join(", ")
}

/// The host-key types to ask the server for: russh's default list, with the
/// types `known_hosts` already holds for this host moved to the front, in
/// russh's own order among themselves — what OpenSSH does, so that a server
/// with a listed key presents *that* key rather than a newer type nobody has
/// verified. A host `known_hosts` does not list keeps the default order.
fn host_key_order(listed: &[(usize, ssh_key::PublicKey)]) -> Vec<ssh_key::Algorithm> {
    let is_listed =
        |a: &ssh_key::Algorithm| listed.iter().any(|(_, k)| same_key_type(&k.algorithm(), a));
    let (first, rest): (Vec<_>, Vec<_>) = russh::Preferred::DEFAULT
        .key
        .iter()
        .cloned()
        .partition(|a| is_listed(a));
    first.into_iter().chain(rest).collect()
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
///
/// `types` is the offered key's type and the types `known_hosts` lists the
/// host under, which only [`HostKeyVerdict::OpenSshOtherType`] reports.
fn refusal_message(
    verdict: HostKeyVerdict,
    host_port: &str,
    known: Option<&str>,
    offered: &str,
    types: (&str, &str),
) -> Option<crate::SshFailure> {
    match verdict {
        HostKeyVerdict::Accept | HostKeyVerdict::RecordAndAccept => None,
        HostKeyVerdict::OpenSshChanged => Some(crate::SshFailure::OpenSshHostKeyChanged {
            host_port: host_port.to_string(),
            offered: offered.to_string(),
        }),
        HostKeyVerdict::OpenSshOtherType => Some(crate::SshFailure::OpenSshKeyTypeUnlisted {
            host_port: host_port.to_string(),
            offered: offered.to_string(),
            offered_type: types.0.to_string(),
            listed_types: types.1.to_string(),
        }),
        HostKeyVerdict::Mismatch => Some(crate::SshFailure::HostKeyChanged {
            host_port: host_port.to_string(),
            known: known.map(str::to_string),
            offered: offered.to_string(),
            store: KNOWN_HOSTS_FILE.to_string(),
        }),
        HostKeyVerdict::StoreUnreadable => Some(crate::SshFailure::HostKeyStoreUnreadable {
            host_port: host_port.to_string(),
            offered: offered.to_string(),
            store: KNOWN_HOSTS_FILE.to_string(),
        }),
    }
}

impl Handler for TunnelClient {
    type Error = russh::Error;

    // OpenSSH's `known_hosts` first (security review S16), then trust-on-first-use
    // host-key verification (review H10): accept and record an unknown host's
    // key; accept a known host only if the key still matches; refuse a changed
    // key (the MITM signal). Fingerprints are SHA256. The composition is
    // `host_key_decision`'s, and pure.
    async fn check_server_key(&mut self, key: &ssh_key::PublicKey) -> Result<bool, Self::Error> {
        let fingerprint = key.fingerprint(ssh_key::HashAlg::Sha256).to_string();
        let openssh = openssh_verdict(&self.listed, key);
        if let Some(OpenSshVerdict::Changed { line }) = openssh {
            tracing::error!(
                "SSH host-key refusal for {}: ~/.ssh/known_hosts line {line} differs",
                self.host_port
            );
        }
        let offered_type = key.algorithm().as_str().to_string();
        let listed = listed_types(&self.listed);
        let types = (offered_type.as_str(), listed.as_str());
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
                    types,
                );
                return Ok(false);
            }
        };
        let loaded: Result<HashMap<String, String>, String> =
            schemaic_core::persist::load_json_strict(KNOWN_HOSTS_FILE);
        let verdict = host_key_decision(
            openssh,
            loaded.as_ref().map_err(String::as_str),
            &self.host_port,
            &fingerprint,
        );
        let known = loaded
            .as_ref()
            .ok()
            .and_then(|s| s.get(&self.host_port))
            .map(String::as_str);
        if let Some(msg) = refusal_message(verdict, &self.host_port, known, &fingerprint, types) {
            // Still logged for anyone running with a console; the connection
            // error is what the user actually sees.
            tracing::error!("SSH host-key refusal for {}: {msg}", self.host_port);
            *self.refusal.lock().unwrap_or_else(|e| e.into_inner()) = Some(msg);
            return Ok(false);
        }
        if verdict == HostKeyVerdict::RecordAndAccept {
            tracing::info!(
                "SSH host {}: recording key {fingerprint} ({})",
                self.host_port,
                if openssh.is_some() {
                    "~/.ssh/known_hosts vouches for it"
                } else {
                    "not seen before, trusted on first use"
                }
            );
            let mut store = loaded.unwrap_or_default();
            store.insert(self.host_port.clone(), fingerprint);
            schemaic_core::persist::save_json(KNOWN_HOSTS_FILE, &store);
        }
        Ok(true)
    }
}

/// Authenticate the freshly connected SSH session per `ssh.auth`, as `user` —
/// the hop's own ([`SshTunnel::route`]), which a jump host may name apart from
/// the tunnel's. Errors carry a human-readable reason (surfaced by the
/// Manage-Connections "Test" button).
async fn authenticate(
    session: &mut client::Handle<TunnelClient>,
    ssh: &SshTunnel,
    user: &str,
) -> Result<(), DbError> {
    let ok = match ssh.auth {
        SshAuth::Password => session
            .authenticate_password(user.to_string(), ssh.password.clone())
            .await
            .map_err(|e| ssh_failed(crate::SshFailure::AuthError(e.to_string())))?
            .success(),
        SshAuth::KeyPair => authenticate_key(session, ssh, user).await?,
        SshAuth::Agent => authenticate_agent(session, user).await?,
    };
    if ok {
        Ok(())
    } else {
        Err(ssh_failed(crate::SshFailure::AuthFailed))
    }
}

/// Private-key-file auth: load the key (decrypting with the passphrase if the
/// key is encrypted), pick the best RSA hash the server advertises, and sign.
async fn authenticate_key(
    session: &mut client::Handle<TunnelClient>,
    ssh: &SshTunnel,
    user: &str,
) -> Result<bool, DbError> {
    let passphrase = (!ssh.key_passphrase.is_empty()).then_some(ssh.key_passphrase.as_str());
    let key = russh::keys::load_secret_key(&ssh.key_path, passphrase)
        .map_err(|e| ssh_failed(crate::SshFailure::KeyLoad(e.to_string())))?;
    let hash_alg = session
        .best_supported_rsa_hash()
        .await
        .ok()
        .flatten()
        .flatten();
    let res = session
        .authenticate_publickey(
            user.to_string(),
            PrivateKeyWithHashAlg::new(Arc::new(key), hash_alg),
        )
        .await
        .map_err(|e| ssh_failed(crate::SshFailure::KeyAuth(e.to_string())))?;
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
        .map_err(|e| ssh_failed(crate::SshFailure::Agent(e.to_string())))?;
    if ids.is_empty() {
        return Err(ssh_failed(crate::SshFailure::AgentEmpty));
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
                .map_err(|e| ssh_failed(crate::SshFailure::AgentAuth(e.to_string())))?;
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
    Err(ssh_failed(crate::SshFailure::NoAgent))
}

#[cfg(unix)]
async fn authenticate_agent(
    session: &mut client::Handle<TunnelClient>,
    user: &str,
) -> Result<bool, DbError> {
    use russh::keys::agent::client::AgentClient;
    let agent = AgentClient::connect_env()
        .await
        .map_err(|e| ssh_failed(crate::SshFailure::NoAgentSocket(e.to_string())))?;
    agent_try(session, user, agent).await
}

/// Why the tunnel cannot carry a connection to `target_host`, or `None`.
///
/// One case: a SQL Server **named instance** (`server\INSTANCE`,
/// [`schemaic_core::connection::sql_server_instance`]). Its port is SQL
/// Server Browser's answer over UDP, and the tunnel forwards TCP alone — so
/// the question cannot be asked through it, and dialled as a name
/// `server\INSTANCE` resolves to nothing. Refused before the SSH sign-in,
/// saying how to reach the instance instead.
pub(crate) fn tunnel_target_refusal(target_host: &str) -> Option<crate::SshFailure> {
    let (server, instance) = schemaic_core::connection::sql_server_instance(target_host)?;
    Some(crate::SshFailure::NamedInstance {
        server: server.to_string(),
        instance: instance.to_string(),
    })
}

/// Open an SSH tunnel to `target_host:target_port` and return a handle carrying
/// the local port the engine's connection should use. The tunnel forwards connections
/// until the handle is dropped.
pub async fn open_tunnel(
    ssh: &SshTunnel,
    target_host: &str,
    target_port: u16,
) -> Result<TunnelHandle, DbError> {
    if let Some(why) = tunnel_target_refusal(target_host) {
        return Err(ssh_failed(why));
    }
    // Keepalives so a dropped SSH session is detected instead of the local port
    // being reused against a dead tunnel forever (review H9). Per hop, because
    // the host-key types asked for are that hop's (`host_key_order`).
    let config_for = |listed: &[(usize, ssh_key::PublicKey)]| {
        let mut preferred = russh::Preferred::default();
        preferred.key = host_key_order(listed).into();
        Arc::new(client::Config {
            keepalive_interval: Some(Duration::from_secs(30)),
            keepalive_max: 3,
            preferred,
            ..Default::default()
        })
    };
    // Every SSH server on the way, jump hosts first (`SshTunnel::route`). The
    // first is dialled over TCP; each later one over a `direct-tcpip` channel
    // of the one before it, as `ssh -J` does — so the database's host need be
    // reachable only from the last, and each hop's host key is checked, on its
    // own `host:port`, exactly as a direct tunnel's is.
    // An alias in `~/.ssh/config` resolves as `ssh <alias>` would — the SSH
    // host and each jump host alike — with what the connection states winning
    // over the file (`ssh_config::resolve_route`).
    let user_config = ssh_config::read_user_config();
    let ssh_config::Route {
        tunnel: resolved,
        hops: route,
    } = ssh_config::resolve_route(
        ssh,
        user_config
            .as_ref()
            .map(|(text, home)| (text.as_str(), home.as_deref())),
    )
    .map_err(|why| ssh_failed(crate::SshFailure::Route(why)))?;
    let ssh = &resolved;
    let mut sessions: Vec<client::Handle<TunnelClient>> = Vec::with_capacity(route.len());
    for hop in &route {
        let host_port = format!("{}:{}", hop.host, hop.port);
        let refusal: Arc<Mutex<Option<crate::SshFailure>>> = Arc::new(Mutex::new(None));
        // A file that cannot be read lists nothing, as before: Schemaic's own
        // store decides. (russh reads it whole, so one line it cannot parse
        // does the same.)
        let listed =
            russh::keys::known_hosts::known_host_keys(&hop.host, hop.port).unwrap_or_default();
        let config = config_for(&listed);
        let handler = TunnelClient {
            host_port: host_port.clone(),
            listed,
            refusal: refusal.clone(),
        };
        let via = sessions.last();
        let connect = async {
            let connected = match via {
                None => client::connect(config, (hop.host.as_str(), hop.port), handler).await,
                Some(prev) => {
                    let channel = prev
                        .channel_open_direct_tcpip(
                            hop.host.clone(),
                            hop.port as u32,
                            "127.0.0.1",
                            0,
                        )
                        .await
                        .map_err(|e| {
                            ssh_failed(crate::SshFailure::JumpUnreachable {
                                host_port: host_port.clone(),
                                error: e.to_string(),
                            })
                        })?;
                    client::connect_stream(config, channel.into_stream(), handler).await
                }
            };
            connected.map_err(|e| {
                // A host-key refusal reaches here as russh's generic `UnknownKey`.
                // Our own verdict is the one worth showing.
                match refusal.lock().ok().and_then(|g| g.clone()) {
                    Some(why) => ssh_failed(why),
                    None if route.len() > 1 => ssh_failed(crate::SshFailure::ConnectTo {
                        host_port: host_port.clone(),
                        error: e.to_string(),
                    }),
                    None => ssh_failed(crate::SshFailure::Connect(e.to_string())),
                }
            })
        };
        let mut session = within(
            TUNNEL_CONNECT_TIMEOUT,
            crate::SshPhase::Connect,
            &host_port,
            connect,
        )
        .await?;
        within(
            TUNNEL_AUTH_TIMEOUT,
            crate::SshPhase::Authentication,
            &host_port,
            authenticate(&mut session, ssh, &hop.user),
        )
        .await?;
        sessions.push(session);
    }
    let session = sessions
        .pop()
        .ok_or_else(|| ssh_failed(crate::SshFailure::NoHost))?;
    // The jump hosts' sessions carry the last one's transport, so they live as
    // long as the tunnel does — moved into the accept loop below.
    let jumps = sessions;

    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .map_err(|e| ssh_failed(crate::SshFailure::Bind(e.to_string())))?;
    let local_port = listener
        .local_addr()
        .map_err(|e| DbError::Connect(e.to_string()))?
        .port();

    tracing::info!(
        "SSH tunnel up: 127.0.0.1:{local_port} → {target_host}:{target_port} via {}",
        route
            .iter()
            .map(|h| format!("{}@{}:{}", h.user, h.host, h.port))
            .collect::<Vec<_>>()
            .join(" → ")
    );

    let session = Arc::new(session);
    let target_host = target_host.to_string();
    let accept = tokio::spawn(async move {
        // Held, not used: dropping a jump host's session would cut the
        // transport every later hop runs over.
        let _jumps = jumps;
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
    use super::{OpenSshVerdict, host_key_decision, host_key_order, listed_types, openssh_verdict};
    use super::{TUNNEL_AUTH_TIMEOUT, TUNNEL_CONNECT_TIMEOUT, ssh_failed, within};
    use crate::DbError;
    use std::collections::HashMap;
    use std::time::Duration;

    /// **A named instance is refused before the tunnel opens**, saying why —
    /// its port is SQL Server Browser's answer over UDP — and how to reach it;
    /// a plain host is the tunnel's to carry. Asked of `open_tunnel` itself,
    /// so the refusal is the one every caller gets, and before any I/O.
    #[tokio::test]
    async fn a_named_instance_is_refused_through_a_tunnel() {
        let refused = super::tunnel_target_refusal("db.corp\\SQLEXPRESS").unwrap();
        let why = refused.to_string();
        assert!(why.contains("UDP") && why.contains("db.corp"), "{why}");
        assert!(super::tunnel_target_refusal("db.corp").is_none());
        let ssh = schemaic_core::connection::SshTunnel {
            host: "ssh.invalid".into(),
            ..Default::default()
        };
        match super::open_tunnel(&ssh, "db.corp\\SQLEXPRESS", 1433).await {
            Err(DbError::ConnectFailed(crate::ConnectFailure::Ssh(f))) => assert_eq!(f, refused),
            Err(e) => panic!("{e}"),
            Ok(_) => panic!("a tunnel opened to a named instance"),
        }
    }

    /// A server that accepts the connection and never answers is refused once the
    /// bound passes, naming the phase and the host — not awaited forever.
    #[tokio::test(start_paused = true)]
    async fn a_silent_ssh_server_is_given_up_on_at_the_bound() {
        let silent = std::future::pending::<Result<(), DbError>>();
        let got = within(
            TUNNEL_CONNECT_TIMEOUT,
            crate::SshPhase::Connect,
            "bastion:22",
            silent,
        )
        .await;
        match got {
            Err(e @ DbError::ConnectFailed(_)) => {
                assert_eq!(
                    e.to_string(),
                    "connection failed: SSH connect with bastion:22: no answer within 20s"
                );
            }
            other => panic!("expected a connect refusal, got {other:?}"),
        }
    }

    /// An answer inside the bound comes back as it was — including the phase's
    /// own error, which must not be replaced by a timeout message.
    #[tokio::test(start_paused = true)]
    async fn an_answer_inside_the_bound_is_passed_through() {
        let ok = within(
            TUNNEL_CONNECT_TIMEOUT,
            crate::SshPhase::Connect,
            "h:22",
            async {
                tokio::time::sleep(Duration::from_secs(19)).await;
                Ok::<_, DbError>(5)
            },
        )
        .await;
        assert_eq!(ok.unwrap(), 5);
        let refused = within(
            TUNNEL_AUTH_TIMEOUT,
            crate::SshPhase::Authentication,
            "h:22",
            async { Err::<(), _>(ssh_failed(crate::SshFailure::AuthFailed)) },
        )
        .await;
        match refused {
            Err(DbError::ConnectFailed(crate::ConnectFailure::Ssh(f))) => {
                assert_eq!(f.to_string(), "SSH authentication failed")
            }
            other => panic!("expected the auth refusal itself, got {other:?}"),
        }
    }

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
            ("", ""),
        )
        .expect("a refusal must be explainable")
        .to_string();
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
            ("", ""),
        )
        .expect("a refusal must be explainable")
        .to_string();
        assert!(!msg.contains("CHANGED"), "{msg}");
        assert!(msg.contains(super::KNOWN_HOSTS_FILE), "{msg}");
    }

    /// Throwaway public keys, made with `ssh-keygen` for these tests alone.
    const ED25519_A: &str =
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIJOyoUorGIUOSsuhZzBRy6bq9keEmDSlyLaNmnYOkDe+";
    const ED25519_B: &str =
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIEjGGxxO+yW8ySSgwb1L8VpavZoMkH1IJ/fWI6oDckvC";
    const ECDSA_P256: &str = "ecdsa-sha2-nistp256 AAAAE2VjZHNhLXNoYTItbmlzdHAyNTYAAAAIbmlzdHAyNTYAAABBBMPFoHp28GtqqNzjo0AeO8Msrrmkm4Nt8F8RSP5CV6YxES90ReiEIMSYyBIgohEu9ewoAQDBjX3YnLpNfLj18v0=";
    const RSA: &str = "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAAAgQDgDhUIYiI5V/KnCegKCXXTl9SwpAp6og+mBQnkMr9yx1DCakox80jWUHMaHywi1MmXSOvpndIJQt8LGMDPoCTebDVn1kalu2khLIsTs3gPv/kcKqTfLWjA3PsuMVP6A/pTnAD7a7tYW1OPJmyrTi5YxGQSWKyt/8vGTe52TyeyZQ==";

    fn key(openssh: &str) -> ssh_key::PublicKey {
        ssh_key::PublicKey::from_openssh(openssh).unwrap()
    }

    fn fp(openssh: &str) -> String {
        key(openssh)
            .fingerprint(ssh_key::HashAlg::Sha256)
            .to_string()
    }

    /// The whole decision for `offered`, as `check_server_key` composes it.
    fn decide(
        listed: &[(usize, &str)],
        store: &HashMap<String, String>,
        offered: &str,
    ) -> HostKeyVerdict {
        let listed: Vec<_> = listed.iter().map(|(l, k)| (*l, key(k))).collect();
        host_key_decision(
            openssh_verdict(&listed, &key(offered)),
            Ok(store),
            "bastion:22",
            &fp(offered),
        )
    }

    /// **A host `known_hosts` vouches for is pinned, and a key of a type it
    /// does not list is refused** (release review S2-L5-01, Critical). The
    /// vouched host was accepted without touching Schemaic's store, so an
    /// attacker later offering ECDSA — which `known_hosts`, listing ed25519
    /// alone, could not judge — fell through to an empty store and was
    /// trusted on first use, and the tunnel's password went to them.
    #[test]
    fn a_key_type_downgrade_on_a_host_known_hosts_lists_is_refused() {
        let empty = store(&[]);
        let listed = [(3, ED25519_A)];
        // The real server, first connect: accepted, and pinned.
        assert_eq!(
            decide(&listed, &empty, ED25519_A),
            HostKeyVerdict::RecordAndAccept
        );
        // The interception, on any connect: refused, store or no store.
        assert_eq!(
            decide(&listed, &empty, ECDSA_P256),
            HostKeyVerdict::OpenSshOtherType
        );
        let pinned = store(&[("bastion:22", &fp(ED25519_A))]);
        assert_eq!(
            decide(&listed, &pinned, ECDSA_P256),
            HostKeyVerdict::OpenSshOtherType
        );
        // And once pinned, a vouched-for key is simply accepted.
        assert_eq!(decide(&listed, &pinned, ED25519_A), HostKeyVerdict::Accept);
        // With `known_hosts` silent, the pin alone refuses the other type.
        assert_eq!(decide(&[], &pinned, ECDSA_P256), HostKeyVerdict::Mismatch);
    }

    /// **A host listed twice under one type accepts either key** (release
    /// review S2-L1-02), as OpenSSH does after a rotation; russh's own check
    /// stopped at the first same-type line that differed and called it an
    /// interception. A key on no line, with a same-type line, is still the
    /// changed key, naming the first such line.
    #[test]
    fn a_rotated_host_listed_twice_accepts_the_newer_key() {
        let listed = [(3, ED25519_A), (9, ED25519_B)];
        let listed: Vec<_> = listed.iter().map(|(l, k)| (*l, key(k))).collect();
        assert_eq!(
            openssh_verdict(&listed, &key(ED25519_B)),
            Some(OpenSshVerdict::Known)
        );
        assert_eq!(
            openssh_verdict(&listed, &key(ED25519_A)),
            Some(OpenSshVerdict::Known)
        );
        let one = [(4, key(ED25519_A))];
        assert_eq!(
            openssh_verdict(&one, &key(ED25519_B)),
            Some(OpenSshVerdict::Changed { line: 4 })
        );
        assert_eq!(
            decide(&[(4, ED25519_A)], &store(&[]), ED25519_B),
            HostKeyVerdict::OpenSshChanged
        );
    }

    /// **The user's own `~/.ssh/known_hosts` is asked first** (security review
    /// S16), and a host it does not list falls through to Schemaic's own
    /// first-use trust — as does a file that cannot be read, which lists
    /// nothing.
    #[test]
    fn a_host_known_hosts_does_not_list_is_trusted_on_first_use() {
        assert_eq!(openssh_verdict(&[], &key(ED25519_A)), None);
        assert_eq!(
            decide(&[], &store(&[]), ED25519_A),
            HostKeyVerdict::RecordAndAccept
        );
    }

    /// `known_hosts` vouching is the user's word over the store's pin: a
    /// rotation recorded there re-pins, and an unreadable store neither blocks
    /// the vouched key nor gets overwritten.
    #[test]
    fn a_vouched_key_supersedes_the_pin_and_survives_an_unreadable_store() {
        let stale = store(&[("bastion:22", &fp(ED25519_A))]);
        assert_eq!(
            decide(&[(1, ED25519_B)], &stale, ED25519_B),
            HostKeyVerdict::RecordAndAccept
        );
        assert_eq!(
            host_key_decision(
                Some(OpenSshVerdict::Known),
                Err("bad json"),
                "bastion:22",
                "SHA256:x"
            ),
            HostKeyVerdict::Accept
        );
    }

    /// **The types `known_hosts` holds are asked for first**, as OpenSSH
    /// does, so a server with a listed key presents that one — which is what
    /// lets the other-type refusal stand without refusing a server that also
    /// has a newer key type. RSA's signature variants count as the one
    /// `ssh-rsa` key; an unlisted host keeps russh's order.
    #[test]
    fn the_listed_key_types_are_asked_for_first() {
        use ssh_key::Algorithm;
        let default = russh::Preferred::DEFAULT.key.to_vec();
        assert_eq!(host_key_order(&[]), default);
        let order = host_key_order(&[(1, key(RSA))]);
        assert_eq!(order.len(), default.len());
        let rsa = |a: &Algorithm| matches!(a, Algorithm::Rsa { .. });
        let first_other = order.iter().position(|a| !rsa(a)).unwrap();
        assert!(first_other > 0 && order[..first_other].iter().all(rsa));
        assert!(order[first_other..].iter().all(|a| !rsa(a)), "{order:?}");
        let order = host_key_order(&[(1, key(ECDSA_P256)), (2, key(ED25519_A))]);
        assert_eq!(
            order[0],
            Algorithm::Ed25519,
            "russh's own order among the listed"
        );
        assert_eq!(
            order[1],
            Algorithm::Ecdsa {
                curve: ssh_key::EcdsaCurve::NistP256
            }
        );
    }

    #[test]
    fn an_other_type_refusal_names_both_types_and_verification_first() {
        let msg = refusal_message(
            HostKeyVerdict::OpenSshOtherType,
            "bastion:22",
            None,
            "SHA256:new",
            ("ecdsa-sha2-nistp256", "ssh-ed25519"),
        )
        .expect("a refusal must be explainable")
        .to_string();
        assert!(
            msg.contains("ecdsa-sha2-nistp256") && msg.contains("ssh-ed25519"),
            "{msg}"
        );
        assert!(
            msg.contains("SHA256:new") && msg.contains("bastion:22"),
            "{msg}"
        );
        let verify = msg
            .find("erify")
            .expect("must ask for out-of-band checking");
        let remedy = msg
            .find("connect with ssh")
            .expect("must say how to re-trust");
        assert!(verify < remedy, "{msg}");
        assert_eq!(
            listed_types(&[(1, key(ED25519_A)), (2, key(RSA)), (3, key(ED25519_B))]),
            "ssh-ed25519, ssh-rsa"
        );
    }

    #[test]
    fn an_accepted_key_has_nothing_to_explain() {
        for v in [HostKeyVerdict::Accept, HostKeyVerdict::RecordAndAccept] {
            assert_eq!(
                refusal_message(v, "h:22", Some("SHA256:a"), "SHA256:a", ("", "")),
                None
            );
        }
    }
}
