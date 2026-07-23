//! Saved connection definitions (server-level), persisted across restarts.
//!
//! A `Connection` is a database *server* (host + credentials), not a single
//! database — the schema sidebar lists all of a connection's databases. An
//! optional SSH tunnel is captured here (password / key-pair / agent auth); it's
//! established by `schemaic_db::ssh::open_tunnel`.
//!
//! NOTE: passwords are stored in plaintext JSON for now. Moving secrets to the
//! OS keychain (`keyring`) is tracked in ARCHITECTURE §16.

use serde::{Deserialize, Serialize};

/// Live reachability of the active connection (health-checked periodically).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ConnStatus {
    /// Not yet checked (or check in flight before any result).
    #[default]
    Unknown,
    /// A recent health check succeeded.
    Connected,
    /// A recent health check failed (unreachable / auth / tunnel down).
    Disconnected,
}

/// How the SSH tunnel authenticates to the jump host.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum SshAuth {
    /// Username + password.
    #[default]
    Password,
    /// A private-key file (optionally passphrase-protected).
    KeyPair,
    /// Delegate signing to the running SSH agent (OpenSSH agent / Pageant on
    /// Windows, `$SSH_AUTH_SOCK` on Unix) — no secret is stored by Schemaic.
    Agent,
}

impl SshAuth {
    /// All variants, in dropdown order.
    pub const ALL: [SshAuth; 3] = [SshAuth::Password, SshAuth::KeyPair, SshAuth::Agent];

    /// Human label for the picker.
    pub fn label(self) -> &'static str {
        match self {
            SshAuth::Password => "Password",
            SshAuth::KeyPair => "Key pair",
            SshAuth::Agent => "SSH agent",
        }
    }
}

/// Optional SSH tunnel for reaching a server that isn't directly routable.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SshTunnel {
    pub enabled: bool,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
    /// Which authentication method to use (default: password, for back-compat
    /// with connections saved before key-pair/agent support).
    #[serde(default)]
    pub auth: SshAuth,
    /// Path to the private-key file (used when `auth == KeyPair`).
    #[serde(default)]
    pub key_path: String,
    /// Passphrase decrypting `key_path`, if the key is encrypted (may be empty).
    #[serde(default)]
    pub key_passphrase: String,
}

impl Default for SshTunnel {
    fn default() -> Self {
        SshTunnel {
            enabled: false,
            host: String::new(),
            port: 22,
            user: String::new(),
            password: String::new(),
            auth: SshAuth::Password,
            key_path: String::new(),
            key_passphrase: String::new(),
        }
    }
}

/// A saved connection to a database server.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Connection {
    pub id: u64,
    pub name: String,
    /// Engine label; only "MySQL" is wired for now.
    #[serde(default = "default_db_type")]
    pub db_type: String,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
    #[serde(default)]
    pub ssh: SshTunnel,
    /// Optional identity colour (a `#rrggbb` hex), shown as a dot across the
    /// connection switcher and the SCHEMA header. `None` = no colour assigned.
    #[serde(default)]
    pub color: Option<String>,
    /// When true, the identity colour is drawn as a prominent frame around the
    /// query+results editor — a guard-rail for production connections. Off by
    /// default.
    #[serde(default)]
    pub prominent_color: bool,
    /// Read-only guard-rail: when true, inline cell edits are disabled and running
    /// any write/DDL statement in the editor is refused. Off by default.
    #[serde(default)]
    pub read_only: bool,
}

fn default_db_type() -> String {
    "MySQL".to_string()
}

impl Connection {
    /// `host:port`, shown in the UI.
    ///
    /// There is deliberately no `mysql://user:pass@host/db` URL builder: the DB
    /// layer takes a [`crate::connection::Connection`] and passes credentials to
    /// the driver structurally (`schemaic_db::Db`), so nothing threads a
    /// plaintext credential URL as identity (review §3.1) and passwords need no
    /// percent-encoding (review B7).
    pub fn endpoint(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_is_host_colon_port() {
        let c = Connection {
            id: 1,
            name: "prod".to_string(),
            db_type: "MySQL".to_string(),
            host: "db.example.com".to_string(),
            port: 3307,
            user: "root".to_string(),
            password: "secret".to_string(),
            ssh: SshTunnel::default(),
            color: None,
            prominent_color: false,
            read_only: false,
        };
        assert_eq!(c.endpoint(), "db.example.com:3307");
    }

    #[test]
    fn ssh_auth_labels_and_all_cover_every_variant() {
        assert_eq!(SshAuth::ALL.len(), 3);
        assert_eq!(SshAuth::Password.label(), "Password");
        assert_eq!(SshAuth::KeyPair.label(), "Key pair");
        assert_eq!(SshAuth::Agent.label(), "SSH agent");
        assert_eq!(SshAuth::default(), SshAuth::Password);
    }

    #[test]
    fn ssh_tunnel_default_uses_port_22_and_password_auth() {
        let t = SshTunnel::default();
        assert_eq!(t.port, 22);
        assert!(!t.enabled);
        assert_eq!(t.auth, SshAuth::Password);
    }

    #[test]
    fn connection_deserializes_with_backcompat_defaults() {
        // A connection saved before db_type/ssh/color/read_only existed.
        let json = r#"{
            "id": 7,
            "name": "legacy",
            "host": "127.0.0.1",
            "port": 3306,
            "user": "app",
            "password": ""
        }"#;
        let c: Connection = serde_json::from_str(json).unwrap();
        assert_eq!(c.db_type, "MySQL");
        assert_eq!(c.ssh, SshTunnel::default());
        assert_eq!(c.color, None);
        assert!(!c.prominent_color);
        assert!(!c.read_only);
    }

    #[test]
    fn ssh_tunnel_deserializes_with_auth_defaults() {
        // Saved before key-pair/agent auth: no auth/key_path/key_passphrase.
        let json = r#"{
            "enabled": true,
            "host": "jump",
            "port": 22,
            "user": "me",
            "password": "pw"
        }"#;
        let t: SshTunnel = serde_json::from_str(json).unwrap();
        assert_eq!(t.auth, SshAuth::Password);
        assert_eq!(t.key_path, "");
        assert_eq!(t.key_passphrase, "");
    }
}
