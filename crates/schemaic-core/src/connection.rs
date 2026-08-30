//! Saved connection definitions (server-level), persisted across restarts.
//!
//! A `Connection` is a database *server* (host + credentials), not a single
//! database — the schema sidebar lists all of a connection's databases. An
//! optional SSH tunnel is captured here (password / key-pair / agent auth); it's
//! established by `schemaic_db::ssh::open_tunnel`.
//!
//! **SQLite is the exception to all of that**, and it is worth stating once here
//! rather than at each field: there is no server, so [`Connection::file`] is the
//! whole target and `host`/`port`/`user`/`password`/`ssh` are inert. It follows
//! that such a connection has no secret to keep (nothing reaches the keyring — an
//! empty password is deleted from the store, not written), no tunnel to open, and
//! exactly one database, which SQLite itself calls `main`.
//!
//! NOTE: secrets (the DB password, the SSH tunnel password, and the SSH key
//! passphrase) are NOT persisted in this struct's JSON — they live in the OS
//! keyring and are hydrated back into these fields on load. See [`crate::secrets`]
//! for the store seam + migration, and `schemaic-app`'s `secrets` module for the
//! keyring-backed implementation. On a machine with no working keyring the fields
//! fall back to plaintext in the JSON so the app keeps working.

use serde::{Deserialize, Serialize};

/// Live reachability of the active connection, from the last health check.
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

impl ConnStatus {
    /// Should work against this connection be blocked?
    ///
    /// Only a *failed* check blocks. `Unknown` deliberately doesn't: it covers
    /// "not checked yet" and "SSH tunnel still coming up", and treating either
    /// as dead would lock the UI during normal startup. The cost of being wrong
    /// here is a query that fails on its own, which is the status quo.
    pub fn is_down(self) -> bool {
        matches!(self, ConnStatus::Disconnected)
    }
}

/// How the SSH tunnel authenticates to the jump host.
///
/// Deserialized through [`SshAuthRaw`], so a value written by a newer build
/// degrades to the default instead of failing the whole `connections.json` —
/// see [`crate::persist::RightPanelState`] for the full reasoning.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(from = "SshAuthRaw")]
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

/// Parsing shim for [`SshAuth`]; see [`crate::persist::RightPanelState`].
#[derive(Deserialize)]
enum SshAuthRaw {
    Password,
    KeyPair,
    Agent,
    #[serde(other)]
    Unknown,
}

impl From<SshAuthRaw> for SshAuth {
    fn from(raw: SshAuthRaw) -> Self {
        match raw {
            SshAuthRaw::Password => SshAuth::Password,
            SshAuthRaw::KeyPair => SshAuth::KeyPair,
            SshAuthRaw::Agent => SshAuth::Agent,
            SshAuthRaw::Unknown => SshAuth::default(),
        }
    }
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

/// Which environment a connection points at, surfaced as a badge in the top bar
/// so it's always obvious what you're working against.
///
/// Deserialized through [`EnvironmentRaw`]; see
/// [`crate::persist::RightPanelState`] for why every persisted enum has a shim.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(from = "EnvironmentRaw")]
pub enum Environment {
    /// No environment assigned — no badge is shown. The default.
    #[default]
    None,
    /// A database running on the developer's own machine.
    Local,
    /// A shared development server.
    Development,
    /// A QA / test environment.
    Testing,
    /// A pre-production / staging environment.
    Staging,
    /// The live production environment.
    Production,
}

/// Parsing shim for [`Environment`]; see [`crate::persist::RightPanelState`].
#[derive(Deserialize)]
enum EnvironmentRaw {
    None,
    Local,
    Development,
    Testing,
    Staging,
    Production,
    #[serde(other)]
    Unknown,
}

impl From<EnvironmentRaw> for Environment {
    fn from(raw: EnvironmentRaw) -> Self {
        match raw {
            EnvironmentRaw::None => Environment::None,
            EnvironmentRaw::Local => Environment::Local,
            EnvironmentRaw::Development => Environment::Development,
            EnvironmentRaw::Testing => Environment::Testing,
            EnvironmentRaw::Staging => Environment::Staging,
            EnvironmentRaw::Production => Environment::Production,
            EnvironmentRaw::Unknown => Environment::default(),
        }
    }
}

/// How much of **this connection's data** the AI assistant may see.
///
/// One switch over every path that can carry rows off the machine, because
/// three unrelated toggles is how a user ends up believing they are protected
/// while some third path ships samples anyway. The paths it governs are
/// `run_query`, `describe_table`'s sample rows, the grid's attach-to-chat, and
/// the value-sampling behind AI Summary / Fill / Seed.
///
/// It is per connection rather than global because a local scratch database and
/// a client's production server are not the same risk, and a single global
/// answer forces the careless setting on one of them.
///
/// **What it does not offer is masking.** A model cannot tell a masked value
/// from a real one, so it reasons confidently about fiction — and the cases
/// where values matter at all are exactly the ones masking ruins. Send the rows
/// or don't.
///
/// Deserialized through [`AiDataRaw`]; see [`crate::persist::RightPanelState`]
/// for why every persisted enum has a shim.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(from = "AiDataRaw")]
pub enum AiData {
    /// Schema only. No path sends a row: the assistant cannot query or sample,
    /// and the grid's attach actions are refused rather than merely hidden.
    SchemaOnly,
    /// The default. The assistant fetches nothing on its own, but the user may
    /// attach rows from the grid to a question — the gesture *is* the consent,
    /// and it can't be forgotten the way a flipped setting can.
    #[default]
    OnRequest,
    /// The assistant may read rows itself: `run_query`, sample rows in
    /// `describe_table`, and the value samples behind AI Fill / Seed.
    Full,
}

/// Parsing shim for [`AiData`]; see [`crate::persist::RightPanelState`].
#[derive(Deserialize)]
enum AiDataRaw {
    SchemaOnly,
    OnRequest,
    Full,
    #[serde(other)]
    Unknown,
}

impl From<AiDataRaw> for AiData {
    fn from(raw: AiDataRaw) -> Self {
        match raw {
            AiDataRaw::SchemaOnly => AiData::SchemaOnly,
            AiDataRaw::OnRequest => AiData::OnRequest,
            AiDataRaw::Full => AiData::Full,
            // A level written by a newer build means *more* access than this one
            // understands, so it degrades to the default rather than to `Full`.
            AiDataRaw::Unknown => AiData::default(),
        }
    }
}

impl AiData {
    /// All variants, in dropdown order (most restrictive first).
    pub const ALL: [AiData; 3] = [AiData::SchemaOnly, AiData::OnRequest, AiData::Full];

    /// Human label for the picker.
    pub fn label(self) -> &'static str {
        match self {
            AiData::SchemaOnly => "Schema only",
            AiData::OnRequest => "Only what I attach",
            AiData::Full => "Let it read data",
        }
    }

    /// The one-line consequence, shown under the picker. Each says plainly that
    /// rows leave the machine, because that is the fact a label can't carry.
    ///
    /// It says *that* they leave, never who receives them: the recipient is
    /// whichever assistant the CLI is pointed at, so naming one would be copy
    /// that goes stale and reads as an endorsement, while the fact being
    /// consented to is the same either way (`no_level_names_a_vendor`).
    pub fn hint(self) -> &'static str {
        match self {
            // **"Structure", not "names and types".** `describe_table` answers
            // with a table's whole `CREATE`: `DEFAULT` literals, column
            // `COMMENT` text, `CHECK` expressions, an enum's value list and a
            // view's full body. Every one of those is a definition rather than a
            // row, so the *promise* holds — but "names and types only" describes
            // less than what leaves, and this line is the consent gesture.
            AiData::SchemaOnly => {
                "Structure only — names, types and definitions. No row ever leaves this \
                 machine, and attaching is refused."
            }
            AiData::OnRequest => {
                "The assistant reads no data on its own. Rows you attach from a result leave \
                 this machine with that question."
            }
            AiData::Full => {
                "The assistant may run read-only queries and read sample rows by itself. \
                 Whatever it reads leaves this machine."
            }
        }
    }

    /// May the assistant fetch rows **on its own** — `run_query`, and the sample
    /// rows in `describe_table`?
    pub fn may_query(self) -> bool {
        self == AiData::Full
    }

    /// May rows reach the model **at the user's own request** — the grid's
    /// attach-to-chat, and the value samples an AI Summary / Fill / Seed carries?
    ///
    /// True for [`AiData::Full`] too: a level that lets the assistant fetch what
    /// it likes cannot coherently refuse what the user hands it.
    pub fn may_attach(self) -> bool {
        self != AiData::SchemaOnly
    }
}

/// The level a connection saved before [`AiData`] existed should take, given the
/// old global "let the assistant run queries" flag it replaces.
///
/// The flag defaulted to *on*, so most upgrades land on [`AiData::Full`] — the
/// access those users already had, kept rather than silently withdrawn. Turning
/// it off never meant "and don't let me attach anything either", which nothing
/// could do at the time, so the off case lands on the ordinary default.
pub fn migrated_ai_data(legacy_run_queries: bool) -> AiData {
    if legacy_run_queries {
        AiData::Full
    } else {
        AiData::OnRequest
    }
}

impl Environment {
    /// All variants, in dropdown order (unset first).
    pub const ALL: [Environment; 6] = [
        Environment::None,
        Environment::Local,
        Environment::Development,
        Environment::Testing,
        Environment::Staging,
        Environment::Production,
    ];

    /// Human label for the picker.
    pub fn label(self) -> &'static str {
        match self {
            Environment::None => "None",
            Environment::Local => "Local",
            Environment::Development => "Development",
            Environment::Testing => "Testing",
            Environment::Staging => "Staging",
            Environment::Production => "Production",
        }
    }

    /// The text shown on the top-bar badge, or `None` when no badge should show.
    pub fn badge_label(self) -> Option<&'static str> {
        match self {
            Environment::None => None,
            other => Some(other.label()),
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

/// How hard a connection insists on TLS, and how far it verifies the server.
///
/// Five modes rather than one "use SSL" checkbox, because "is this connection
/// safe" is two independent questions — are the bytes encrypted, and is the peer
/// who it claims to be — and a checkbox answers only the first. That is the
/// setting that makes a session *look* protected while anything presenting any
/// self-signed certificate is reading it.
///
/// The names and the semantics are libpq's `sslmode`, deliberately: they are
/// what the hosted providers document, so a user moving a connection string over
/// already has the answer in hand rather than a mapping to guess at.
///
/// Ask one of the four predicates below rather than matching a variant at the
/// call site — [`Self::negotiates_tls`], [`Self::requires_tls`],
/// [`Self::verifies_certificate`], [`Self::verifies_hostname`]. A `mode ==
/// VerifyFull` compiles cleanly while silently sorting a mode added later onto
/// whichever side it happens to fall.
///
/// Deserialized through [`SslModeRaw`], so a value written by a newer build
/// degrades to the default instead of failing the whole `connections.json` —
/// see [`crate::persist::RightPanelState`] for the full reasoning.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(from = "SslModeRaw")]
pub enum SslMode {
    /// Never negotiate TLS; connect in plaintext.
    Disable,
    /// Encrypt when the server offers it, fall back to plaintext when it does
    /// not, and verify nothing.
    ///
    /// The default, and the reason it can be: it never refuses a server that a
    /// plaintext connection would have reached, so every connection saved before
    /// this setting existed keeps working while gaining encryption wherever the
    /// server already offered it.
    #[default]
    Prefer,
    /// Encryption is mandatory — fail rather than fall back — but the
    /// certificate is not checked.
    Require,
    /// Mandatory encryption, and the certificate must chain to a trusted CA.
    VerifyCa,
    /// Mandatory encryption, a trusted chain, **and** a certificate that names
    /// the host we asked for.
    VerifyFull,
}

/// Parsing shim for [`SslMode`]; see [`crate::persist::RightPanelState`].
#[derive(Deserialize)]
enum SslModeRaw {
    Disable,
    Prefer,
    Require,
    VerifyCa,
    VerifyFull,
    #[serde(other)]
    Unknown,
}

impl From<SslModeRaw> for SslMode {
    fn from(raw: SslModeRaw) -> Self {
        match raw {
            SslModeRaw::Disable => SslMode::Disable,
            SslModeRaw::Prefer => SslMode::Prefer,
            SslModeRaw::Require => SslMode::Require,
            SslModeRaw::VerifyCa => SslMode::VerifyCa,
            SslModeRaw::VerifyFull => SslMode::VerifyFull,
            SslModeRaw::Unknown => SslMode::default(),
        }
    }
}

impl SslMode {
    /// All variants, in dropdown order — weakest first, so the list reads as the
    /// ladder it is.
    pub const ALL: [SslMode; 5] = [
        SslMode::Disable,
        SslMode::Prefer,
        SslMode::Require,
        SslMode::VerifyCa,
        SslMode::VerifyFull,
    ];

    /// Human label for the picker.
    pub fn label(self) -> &'static str {
        match self {
            SslMode::Disable => "Disable",
            SslMode::Prefer => "Prefer",
            SslMode::Require => "Require",
            SslMode::VerifyCa => "Verify CA",
            SslMode::VerifyFull => "Verify full",
        }
    }

    /// One line under the picker saying what this mode actually protects — the
    /// difference between the last two is the whole reason there are five, and
    /// it is not guessable from their names.
    pub fn description(self) -> &'static str {
        match self {
            SslMode::Disable => "Connect in plain text.",
            SslMode::Prefer => "Encrypt if the server offers it. No verification.",
            SslMode::Require => "Refuse to connect without encryption. No verification.",
            SslMode::VerifyCa => "Encrypt, and check the certificate against the CA.",
            SslMode::VerifyFull => "Encrypt, check the certificate, and check the host name.",
        }
    }

    /// Does this mode attempt TLS at all?
    pub fn negotiates_tls(self) -> bool {
        !matches!(self, SslMode::Disable)
    }

    /// Must the connect **fail** when the server offers no TLS, rather than fall
    /// back to plaintext?
    pub fn requires_tls(self) -> bool {
        matches!(
            self,
            SslMode::Require | SslMode::VerifyCa | SslMode::VerifyFull
        )
    }

    /// Is the server's certificate chain checked against a trusted CA?
    pub fn verifies_certificate(self) -> bool {
        matches!(self, SslMode::VerifyCa | SslMode::VerifyFull)
    }

    /// Is the server's certificate checked against the host name we asked for?
    ///
    /// The rung above [`Self::verifies_certificate`]: a valid certificate for
    /// *some other host*, replayed by whoever holds it, passes the chain check
    /// and fails only this one.
    pub fn verifies_hostname(self) -> bool {
        matches!(self, SslMode::VerifyFull)
    }
}

/// How a connection secures its transport: the [`SslMode`] plus the files that
/// mode needs.
///
/// Beside [`SshTunnel`] rather than inside it — the two are independent ways of
/// protecting the same hop, and a tunnelled connection may still want the server
/// to prove who it is at the far end.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
pub struct Tls {
    #[serde(default)]
    pub mode: SslMode,
    /// PEM file of CA certificates to trust. Empty means the bundled public
    /// roots, which is what a hosted provider with a publicly-signed certificate
    /// needs — see [`Self::ca_file`].
    #[serde(default)]
    pub ca_path: String,
    /// Client certificate offered to the server (mutual TLS), PEM.
    #[serde(default)]
    pub client_cert_path: String,
    /// Private key for `client_cert_path`, PEM.
    #[serde(default)]
    pub client_key_path: String,
    /// Passphrase decrypting `client_key_path`, if the key is encrypted (may be
    /// empty). A secret: it lives in the OS keyring, not in `connections.json`
    /// — see [`crate::secrets::SecretKind::TlsKeyPassphrase`].
    #[serde(default)]
    pub client_key_passphrase: String,
}

impl Tls {
    /// Should the handshake offer a client certificate?
    ///
    /// **Both halves, and a mode that handshakes at all.** A certificate without
    /// its key cannot answer the server's challenge, so half a pair is not a
    /// weaker identity but a failed connect — and an identity offered during a
    /// handshake means nothing on a connection that never performs one.
    pub fn uses_client_cert(&self) -> bool {
        self.mode.negotiates_tls()
            && !self.client_cert_path.is_empty()
            && !self.client_key_path.is_empty()
    }

    /// The CA file this connection verifies against, or `None` for the system
    /// roots.
    ///
    /// `None` covers two different situations on purpose, because they need the
    /// same thing from the caller: a mode that verifies nothing, and a verifying
    /// mode with no file named. Reading the second as a missing path instead
    /// would break `verify-full` against precisely the hosted servers whose
    /// certificates are already publicly signed.
    pub fn ca_file(&self) -> Option<&str> {
        if !self.mode.verifies_certificate() || self.ca_path.is_empty() {
            return None;
        }
        Some(&self.ca_path)
    }

    /// How the handshake should be performed, or `None` for "connect in
    /// plaintext" — the whole of this setting, resolved once.
    ///
    /// The drivers speak different dialects of the same four decisions (MySQL
    /// takes two `danger_*` toggles on an `SslOpts`; Postgres takes an `SslMode`
    /// plus a rustls verifier), and translating five modes into each of them
    /// separately is how `verify-ca` ends up meaning one thing on one engine and
    /// another on the other. They translate a [`TlsPlan`] instead, so the
    /// *decisions* are made here, once, where they are unit-tested without a
    /// server.
    pub fn plan(&self) -> Option<TlsPlan> {
        if !self.mode.negotiates_tls() {
            return None;
        }
        Some(TlsPlan {
            fallback_to_plaintext: !self.mode.requires_tls(),
            accept_invalid_certs: !self.mode.verifies_certificate(),
            skip_hostname_check: !self.mode.verifies_hostname(),
            root_ca: self.ca_file().map(str::to_string),
            client_identity: self
                .uses_client_cert()
                .then(|| (self.client_cert_path.clone(), self.client_key_path.clone())),
            hostname_override: None,
        })
    }
}

/// A resolved [`Tls`] — what a driver is actually configured from.
///
/// Deliberately *not* the mode: by the time a driver sees this, the five modes
/// have already collapsed into the four independent decisions a handshake is
/// made of, so no driver has to re-derive "does verify-ca check the hostname"
/// and no driver can get a different answer.
/// Serializable because the MCP subprocess is handed its endpoint as a JSON blob
/// over the environment, and a handoff that dropped this would run the user's
/// own queries over plaintext against a server they configured for TLS.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TlsPlan {
    /// May the connection be retried in plaintext when TLS is unavailable?
    /// True for [`SslMode::Prefer`] and nothing else.
    pub fallback_to_plaintext: bool,
    /// Accept a certificate that does not chain to a trusted root.
    pub accept_invalid_certs: bool,
    /// Accept a certificate that does not name the host we asked for.
    pub skip_hostname_check: bool,
    /// PEM file of roots to trust; `None` means the bundled public root set —
    /// **not** the operating system's certificate store, so a CA trusted
    /// machine-wide still has to be named by path.
    pub root_ca: Option<String>,
    /// `(certificate, key)` paths to offer as this client's identity.
    pub client_identity: Option<(String, String)>,
    /// The name the certificate must be *checked against*, when that is not the
    /// host being dialled.
    ///
    /// Exists for one situation, and it is not exotic: an SSH tunnel rewrites the
    /// endpoint to `127.0.0.1:<local port>`, so `verify-full` would compare the
    /// server's certificate against `127.0.0.1` and reject a perfectly good
    /// certificate naming the real host. Set by whoever performs the rewrite, in
    /// the same step — see `schemaic_db::Db::connect`.
    pub hostname_override: Option<String>,
}

/// A saved connection to a database server.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Connection {
    pub id: u64,
    pub name: String,
    /// Engine label — `MySQL`/`MariaDB`, `PostgreSQL` or `SQLite`, read through
    /// [`is_postgres`]/[`is_sqlite`] rather than compared here, since the aliases
    /// are theirs to know. Anything unrecognised is MySQL, which is what keeps a
    /// connection saved before the field existed working.
    #[serde(default = "default_db_type")]
    pub db_type: String,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
    /// The database **file**, for the one engine that has no server:
    /// [`is_sqlite`]. Empty on every other engine, and empty on every connection
    /// saved before this field existed — which is why it is `#[serde(default)]`
    /// and a `String` rather than an `Option`, matching how `host`/`user` already
    /// spell "not set" here.
    ///
    /// It sits beside the server coordinates rather than replacing them because a
    /// connection's engine is editable in place: switching a saved connection from
    /// MySQL to SQLite and back must not discard the host it is going back to.
    #[serde(default)]
    pub file: String,
    #[serde(default)]
    pub ssh: SshTunnel,
    /// How the transport to the server is secured ([`Tls`]). Defaults to
    /// [`SslMode::Prefer`], which is what makes the field safe to add to
    /// connections saved before it existed.
    #[serde(default)]
    pub tls: Tls,
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
    /// Which environment this connection points at (Development / Testing /
    /// Production / …), shown as a badge in the top bar. Defaults to none.
    #[serde(default)]
    pub environment: Environment,
    /// How much of this connection's data the AI assistant may see
    /// ([`AiData`]).
    ///
    /// `None` means "never chosen here" — a connection saved before the setting
    /// existed. The app resolves that once at startup from the old global
    /// `ai_run_queries` flag, so an upgrade neither silently grants the
    /// assistant more access nor silently takes away what the user had.
    #[serde(default)]
    pub ai_data: Option<AiData>,
}

fn default_db_type() -> String {
    "MySQL".to_string()
}

impl Connection {
    /// `host:port`, shown in the UI — or, on SQLite, the file's **name**.
    ///
    /// There is deliberately no `mysql://user:pass@host/db` URL builder: the DB
    /// layer takes a [`crate::connection::Connection`] and passes credentials to
    /// the driver structurally (`schemaic_db::Db`), so nothing threads a
    /// plaintext credential URL as identity (review §3.1) and passwords need no
    /// percent-encoding (review B7).
    ///
    /// A SQLite connection has no host and no port, so `host:port` there would
    /// read `:0` — a coordinate that looks like a misconfiguration. It shows the
    /// last path component instead: this string is a *subtitle* under the
    /// connection's own name in a narrow list, and a full path is both too long
    /// for it and, on a work machine, often the one part of the row nobody wants
    /// to read out. [`Self::file_label`] is the full path, for places with room.
    pub fn endpoint(&self) -> String {
        if is_sqlite(&self.db_type) {
            return file_name(&self.file).to_string();
        }
        format!("{}:{}", self.host, self.port)
    }

    /// This connection as it should be **stored**: a file connection carries no
    /// server coordinates.
    ///
    /// **The engine picker can be changed on a saved connection**, and the SQLite
    /// form renders no host, user, password or SSH block — so a connection
    /// switched over from a tunnelled MySQL one kept the whole server side, with
    /// no control anywhere that could unset any of it. [`Self::uses_tunnel`]
    /// makes the leftover SSH toggle harmless; this is what stops it being there
    /// at all, and what makes this module's opening claim — "there is no server,
    /// so `host`/`port`/`user`/`password`/`ssh` are inert … it has no secret to
    /// keep" — true of what actually reaches `connections.json` and the keyring.
    ///
    /// A no-op on every other engine.
    pub fn sanitized(mut self) -> Connection {
        if is_sqlite(&self.db_type) {
            self.host = String::new();
            self.port = 0;
            self.user = String::new();
            self.password = String::new();
            self.ssh = SshTunnel::default();
            self.tls = Tls::default();
        }
        self
    }

    /// Should opening this connection open an SSH tunnel first?
    ///
    /// **Not `ssh.enabled` on its own.** The engine picker can be changed on a
    /// saved connection, and the SQLite form renders no SSH block at all — so a
    /// connection switched over from a tunnelled MySQL one keeps `ssh.enabled`
    /// set with no control anywhere that can unset it. Asking the flag alone
    /// then made every operation on a purely local file dial a third-party
    /// bastion with a stored credential, forward it to the file connection's
    /// inert `"":0`, and fail the whole connection if that host was down. The
    /// form meanwhile tells the user in as many words that there is "no server
    /// to reach — no host, user, password or tunnel".
    ///
    /// One answer, asked everywhere: [`crate::connection`]'s five tunnel sites,
    /// rather than a sixth spelling of it at each.
    pub fn uses_tunnel(&self) -> bool {
        self.ssh.enabled && !is_sqlite(&self.db_type)
    }

    /// Should opening this connection negotiate TLS?
    ///
    /// **Not `tls.mode.negotiates_tls()` on its own**, and for the same reason
    /// [`Self::uses_tunnel`] is not `ssh.enabled` on its own: the engine picker
    /// is editable in place and the SQLite form renders no TLS section, so a
    /// connection switched over from a MySQL one keeps whatever mode it had with
    /// no control anywhere that could unset it. SQLite is a local file with no
    /// transport to secure, so the question is settled before the mode is asked.
    ///
    /// One answer, asked everywhere, rather than a second spelling of it at each
    /// driver.
    pub fn uses_tls(&self) -> bool {
        self.tls.mode.negotiates_tls() && !is_sqlite(&self.db_type)
    }

    /// How this connection's handshake should be performed, or `None` for
    /// plaintext — [`Tls::plan`] asked through [`Self::uses_tls`], so the engine
    /// settles it before the mode is consulted.
    ///
    /// **The one entry point for the drivers.** Reaching for `conn.tls.plan()`
    /// instead would plan a handshake for a SQLite file.
    pub fn tls_plan(&self) -> Option<TlsPlan> {
        self.uses_tls().then(|| self.tls.plan()).flatten()
    }

    /// The whole path of a SQLite connection's file, empty for any other engine.
    /// For a tooltip or a form, where [`Self::endpoint`]'s short name isn't enough
    /// to tell two files of the same name apart.
    pub fn file_label(&self) -> &str {
        if is_sqlite(&self.db_type) {
            &self.file
        } else {
            ""
        }
    }

    /// A copy of this connection ready to be saved as a new one: everything
    /// that says **how to reach the server** is carried, and only what
    /// identifies the connection *to the user* — its id, name and colour — is
    /// replaced by what the caller supplies.
    ///
    /// The credentials are the point. They live in the OS keyring (see
    /// [`crate::secrets`]), so the alternative to this is retyping a password
    /// and a whole SSH block to reach a second database on the same host. The
    /// guard-rails come across for the opposite reason: a copy of a read-only
    /// production connection that opened writable would be a worse trap than
    /// having no duplicate at all.
    ///
    /// Written as a struct update rather than a field-by-field build, so a
    /// field added to [`Connection`] later is carried **by construction** — the
    /// failure mode here is a new credential silently not being copied, which
    /// no compiler error would catch in the explicit form.
    pub fn duplicate(&self, id: u64, name: String, color: Option<String>) -> Connection {
        Connection {
            id,
            name,
            color,
            ..self.clone()
        }
    }

    /// The id a **new** connection takes: past every one in use.
    ///
    /// One function because the id is the sole component of the keyring account
    /// string (`conn.{id}.password` / `.ssh_password` / `.ssh_passphrase` /
    /// `.tls_key_passphrase`), so
    /// two connections sharing one share all three secret slots — a saved
    /// password appearing under a connection that never had it, and a delete
    /// taking the other's with it. It was written out twice (Save, and
    /// Duplicate) with a comment on the second saying it used "the same rule",
    /// which is the arrangement `ident_sql` exists to rule out: four independent
    /// copies that happened to agree.
    ///
    /// Past the **maximum**, not the count and not the first gap: an id freed by
    /// a delete stays free, because whatever still refers to it — a keyring entry
    /// a `forget` missed, a `history.json` this build didn't write — would
    /// otherwise be adopted by the next connection created.
    pub fn next_id(existing: &[Connection]) -> u64 {
        existing.iter().map(|c| c.id).max().unwrap_or(0) + 1
    }

    /// Which connection is active at startup, given the id the last session
    /// saved and the connections that actually loaded.
    ///
    /// The saved id wins when it still names a connection; otherwise the first
    /// one does, since a list with nothing selected is a worse start than an
    /// arbitrary selection.
    ///
    /// **With no connections at all it answers [`Connection::next_id`] of
    /// nothing** — the id the first connection saved in this session is about to
    /// take. That is not a placeholder: `save_conn` loads the schema for a
    /// connection it saves *only* when it is the active one, so the first
    /// connection a new user creates connects on save rather than sitting there
    /// until they switch to it by hand. Until the seed connection was removed
    /// this arm was unreachable and spelled `unwrap_or(1)`, which happens to be
    /// the same number — the coupling to `next_id` is the part that has to hold,
    /// and it is what this states.
    pub fn startup_active_id(saved: Option<u64>, connections: &[Connection]) -> u64 {
        saved
            .filter(|id| connections.iter().any(|c| c.id == *id))
            .or_else(|| connections.first().map(|c| c.id))
            .unwrap_or_else(|| Connection::next_id(&[]))
    }

    /// Do these two point at the same server — everything that decides *which*
    /// server the next query reaches, and nothing else?
    ///
    /// The schema tree asks this to decide whether a reload may keep the
    /// databases it is already showing (see `SchemaState::begin_refresh`) or has
    /// to clear them. Same server → the rows stay up while they refresh. A
    /// different one → they are another server's and must go, even though the
    /// saved connection kept its id, which is the case an id comparison alone
    /// gets wrong: editing a connection's host is an edit *in place*.
    ///
    /// The password is deliberately not part of it. A corrected password reaches
    /// the same databases; treating it as a different server would blank the
    /// tree for a change that moves nothing. Nor is anything presentational
    /// (name, colour, environment) or a guard-rail (`read_only`) — those don't
    /// change what the server holds.
    /// On SQLite the file **is** the server, so it joins the comparison: pointing
    /// a saved connection at another `.db` reaches an entirely different set of
    /// tables, which is the case this exists to catch, and it is reached by
    /// editing a connection in place exactly as a repointed host is.
    ///
    /// **`id` is deliberately not part of it**, and used to be. A connection's
    /// id decides nothing about which server a query reaches — it is *caller
    /// policy*, belonging to the one caller that means "is this a reload of the
    /// same connection", which is now [`Connection::is_reload_of`]. Encoding it
    /// here made the function answer a different question from the one its name
    /// and this doc ask, two paragraphs below an argument *against* an id
    /// comparison. What that cost: the killed-session repair asks the honest
    /// question — two saved connections routinely point at one server, `local
    /// (app)` and `local (root)`, or two entries differing only in default
    /// database — and is reached only when the ids differ, so it was `false` by
    /// construction and a pinned Manual tab was left holding a dead socket with
    /// Commit and Rollback still offered.
    pub fn targets_same_server(&self, other: &Connection) -> bool {
        self.db_type == other.db_type
            && self.file == other.file
            && self.host == other.host
            && self.port == other.port
            && self.user == other.user
            && self.ssh.enabled == other.ssh.enabled
            && self.ssh.host == other.ssh.host
            && self.ssh.port == other.ssh.port
            && self.ssh.user == other.ssh.user
    }

    /// Is `other` **this same saved connection, still pointing where it did** —
    /// the question the schema tree asks before keeping the databases it is
    /// already showing?
    ///
    /// Two terms, and they are two different things. The id says it is the same
    /// entry in the list rather than a *switch* to another connection, whose
    /// databases would be somebody else's for as long as the connect takes;
    /// [`targets_same_server`](Connection::targets_same_server) says the entry
    /// has not been edited to point somewhere new, which an id comparison alone
    /// gets wrong because repointing a host is an edit *in place*.
    ///
    /// Split out so the two questions cannot be confused again: the killed-
    /// session repair asks the second one about two genuinely different
    /// connections, and while the id term lived inside the shared predicate its
    /// call was dead code.
    pub fn is_reload_of(&self, other: &Connection) -> bool {
        self.id == other.id && self.targets_same_server(other)
    }
}

/// The last path component of `path`, splitting on both separators.
///
/// Both, deliberately: `connections.json` is portable and the app runs on
/// Windows and Linux, so a path saved on one can be read on the other, and
/// `std::path` on Linux does not treat `\` as a separator — a Windows path would
/// come back whole as its own "file name".
fn file_name(path: &str) -> &str {
    match path.rfind(['/', '\\']) {
        Some(i) => &path[i + 1..],
        None => path,
    }
}

#[cfg(test)]
mod status_tests {
    use super::ConnStatus;

    #[test]
    fn only_a_failed_check_blocks_work() {
        assert!(ConnStatus::Disconnected.is_down());
        assert!(!ConnStatus::Connected.is_down());
        // Unknown covers "not checked yet" and "tunnel still coming up" — both
        // normal startup states that must not gate the UI.
        assert!(!ConnStatus::Unknown.is_down());
        assert!(!ConnStatus::default().is_down());
    }
}

/// Is this saved `db_type` label PostgreSQL?
///
/// Anything not recognizably Postgres is MySQL — the historical default, which
/// is what keeps connections saved before the field existed working. **The one
/// answer**: `schemaic_db::Engine::from_db_type` and the connection form's
/// engine picker both read it, so a new label can't mean Postgres to the driver
/// and MySQL to the form.
pub fn is_postgres(db_type: &str) -> bool {
    let t = db_type.trim();
    t.eq_ignore_ascii_case("postgresql")
        || t.eq_ignore_ascii_case("postgres")
        || t.eq_ignore_ascii_case("pg")
}

/// Is this saved `db_type` label SQLite?
///
/// **The one answer**, on the same terms as [`is_postgres`]: `schemaic_db::Engine`,
/// the connection form's picker and `intel::SqlDialect::from_db_type` all read it,
/// so a label can't mean SQLite to the driver and MySQL to the form.
///
/// This is the question most of the app ends up asking, because SQLite is the one
/// engine with **no server behind it**: [`Connection::file`] is the whole target,
/// and `host`/`port`/`user`/`password`/`ssh` mean nothing on such a connection.
pub fn is_sqlite(db_type: &str) -> bool {
    let t = db_type.trim();
    t.eq_ignore_ascii_case("sqlite") || t.eq_ignore_ascii_case("sqlite3")
}

/// Do two `db_type` labels name the **same engine**?
///
/// One engine has more than one label — `MariaDB` and `MySQL` are the same
/// engine, `pg` and `PostgreSQL` are, and an empty label predates the field —
/// so a string comparison is not this question. Answered through
/// [`is_postgres`]/[`is_sqlite`] rather than by a `match` of its own, so a
/// fourth engine cannot sort onto whichever side it happens to fall.
///
/// The connection form's Type picker asks it to tell *its own* change apart from
/// a connection being loaded into the form: on a pick the stored label still
/// names the previous engine, on a load it already names the new one. Only the
/// first should rewrite the label or offer the new engine's default port.
pub fn same_engine(a: &str, b: &str) -> bool {
    is_postgres(a) == is_postgres(b) && is_sqlite(a) == is_sqlite(b)
}

/// How to name a connection's engine on screen.
///
/// Normalises the PostgreSQL aliases [`is_postgres`] accepts onto the one label
/// (a hand-edited `"pg"` shouldn't reach the user), and otherwise shows the label
/// as saved — `MariaDB` is worth saying when that's what it is. An empty one
/// predates the field, and everything from that era was MySQL.
pub fn engine_label(db_type: &str) -> String {
    if is_postgres(db_type) {
        return "PostgreSQL".to_string();
    }
    if is_sqlite(db_type) {
        return "SQLite".to_string();
    }
    match db_type.trim() {
        "" => "MySQL".to_string(),
        other => other.to_string(),
    }
}

/// Is this engine reached **over the network** — i.e. does a host, a port, a
/// user, a password or an SSH tunnel mean anything for it?
///
/// SQLite is the one that answers `false`, and it is a predicate rather than an
/// `is_sqlite(…)` at each site because the *question* is what the callers
/// actually have: whether to open a tunnel, whether to show a port field,
/// whether a credential is worth keyring space.
///
/// **It lives here so there is one answer.** There were two — `Engine::is_networked`
/// in `schemaic-db` and a `DbKind::is_networked` in the connection form — and
/// while the two agreed, having two is what let the *third* consumer, the tunnel
/// decision, quietly not ask at all. Both now delegate here.
pub fn is_networked(db_type: &str) -> bool {
    !is_sqlite(db_type)
}

/// The default TCP port for a `db_type` label.
///
/// Used both when the engine picker changes and as the fallback for a port field
/// that doesn't parse — the second of which used to be a bare `3306`, so clearing
/// a PostgreSQL connection's port and saving pointed it silently at the MySQL
/// port and redisplayed that as though the user had typed it.
///
/// SQLite answers **0**, which is not a port but the absence of one: there is no
/// server to reach, so any real number here would be a coordinate the app might
/// later show or try. The form doesn't build a port field for SQLite at all.
pub fn default_port(db_type: &str) -> u16 {
    if is_sqlite(db_type) {
        return 0;
    }
    if is_postgres(db_type) { 5432 } else { 3306 }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── AI data access ──

    #[test]
    fn only_full_access_lets_the_assistant_fetch_rows_itself() {
        assert!(AiData::Full.may_query());
        assert!(!AiData::OnRequest.may_query());
        assert!(!AiData::SchemaOnly.may_query());
    }

    #[test]
    fn schema_only_refuses_even_a_row_the_user_hands_over() {
        // The point of the strictest level: no path, deliberate or not.
        assert!(!AiData::SchemaOnly.may_attach());
        assert!(AiData::OnRequest.may_attach());
        // A level that lets the assistant read what it likes cannot coherently
        // refuse what the user chose to send.
        assert!(AiData::Full.may_attach());
    }

    #[test]
    fn the_default_grants_no_automatic_access() {
        assert_eq!(AiData::default(), AiData::OnRequest);
        assert!(!AiData::default().may_query());
    }

    /// A newer build's level must degrade to the *safe* one, not to `Full` and
    /// not by failing the whole file (which would lose every connection).
    #[test]
    fn an_unknown_level_degrades_to_the_default() {
        let c: Connection =
            serde_json::from_str(r#"{"id":1,"name":"n","host":"h","port":3306,"user":"u","password":"","ai_data":"ReadEverything"}"#)
                .expect("an unknown level still parses");
        assert_eq!(c.ai_data, Some(AiData::default()));
        assert!(!c.ai_data.unwrap().may_query());
    }

    /// Absent is not the same as default: a connection saved before the setting
    /// existed carries `None`, which the app resolves from the old global flag.
    #[test]
    fn a_connection_saved_before_the_setting_has_no_level_at_all() {
        let c: Connection = serde_json::from_str(
            r#"{"id":1,"name":"n","host":"h","port":3306,"user":"u","password":""}"#,
        )
        .unwrap();
        assert_eq!(c.ai_data, None);
    }

    /// The upgrade must not change what the assistant can reach: someone who had
    /// `run_query` keeps it, someone who had turned it off does not get it back.
    /// The saved id wins while it names something; a stale one falls back to the
    /// first connection rather than to nothing.
    #[test]
    fn startup_keeps_the_last_active_connection_when_it_still_exists() {
        let cs = vec![
            Connection { id: 4, ..conn() },
            Connection { id: 9, ..conn() },
        ];
        assert_eq!(Connection::startup_active_id(Some(9), &cs), 9);
        assert_eq!(Connection::startup_active_id(Some(77), &cs), 4);
        assert_eq!(Connection::startup_active_id(None, &cs), 4);
    }

    /// With nothing saved, the active id is the one the *first* connection
    /// created this session will be given — so saving it makes it active, and
    /// the save path connects it instead of leaving the user on a connection
    /// they have to select by hand. A number picked any other way would break
    /// that silently.
    #[test]
    fn an_empty_list_points_at_the_id_the_first_connection_will_take() {
        assert_eq!(
            Connection::startup_active_id(None, &[]),
            Connection::next_id(&[])
        );
        assert_eq!(Connection::startup_active_id(Some(3), &[]), 1);
        // **Reached mid-session too**, since deleting the last connection lands
        // in the same empty state and answers it the same way. It used to leave
        // `active_conn` on the deleted id, so the connection created next was
        // saved, took the switcher slot, and never connected — `save_conn` loads
        // a schema only for the connection that is active, and the new one takes
        // `next_id(&[])`.
        assert_eq!(Connection::startup_active_id(Some(2), &[]), 1);
    }

    #[test]
    fn the_migration_keeps_the_access_the_old_flag_granted() {
        assert_eq!(migrated_ai_data(true), AiData::Full);
        assert!(migrated_ai_data(true).may_query());
        assert!(!migrated_ai_data(false).may_query());
    }

    #[test]
    fn every_level_says_the_rows_leave_the_machine() {
        // The hint is the consent copy; a level that doesn't name the
        // consequence is the setting people click through. The consequence is
        // that the rows leave the user's machine — which is what they are
        // consenting to, and it is true whichever assistant is on the other end.
        for lvl in AiData::ALL {
            assert!(!lvl.label().is_empty());
            let hint = lvl.hint();
            assert!(hint.contains("this machine"), "{:?}: {hint}", lvl);
        }
        // **A substring check accepts its own negation**, which is how this test
        // could have gone on passing over "Nothing ever leaves this machine" on
        // the two levels where things do. So each level's hint is asserted
        // against its actual answer, not against a phrase.
        for lvl in AiData::ALL {
            let hint = lvl.hint().to_lowercase();
            let says_never = hint.contains("no row ever leaves")
                || hint.contains("nothing ever leaves")
                || hint.contains("never leaves");
            assert_eq!(
                says_never,
                !lvl.may_attach(),
                "{lvl:?} promises the opposite of what it permits: {hint}"
            );
            // …and the two that do send say so in the active voice.
            if lvl.may_attach() {
                assert!(
                    hint.contains("leave this machine") || hint.contains("leaves this machine"),
                    "{lvl:?}: {hint}"
                );
            }
        }
    }

    /// Naming a vendor in the consent line dates it and buys nothing: the fact
    /// that matters is that the rows leave, not who receives them, and Schemaic
    /// is not the place to advertise whoever is behind the CLI it drives.
    #[test]
    fn no_level_names_a_vendor() {
        for lvl in AiData::ALL {
            for vendor in ["Anthropic", "Claude", "OpenAI", "ChatGPT"] {
                assert!(
                    !lvl.hint().contains(vendor) && !lvl.label().contains(vendor),
                    "{:?} names {vendor}",
                    lvl
                );
            }
        }
    }

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
            file: String::new(),
            ssh: SshTunnel::default(),
            tls: Tls::default(),
            color: None,
            prominent_color: false,
            read_only: false,
            environment: Environment::None,
            ai_data: None,
        };
        assert_eq!(c.endpoint(), "db.example.com:3307");
        // A server connection has no file to label.
        assert_eq!(c.file_label(), "");
    }

    /// A SQLite connection has no host and no port, so `host:port` would read
    /// `:0` — a coordinate that looks like a misconfiguration. It shows the file's
    /// name, with the whole path available separately for somewhere with room.
    #[test]
    fn a_sqlite_endpoint_is_the_file_name_not_a_host_and_port() {
        let mut c = conn();
        c.db_type = "SQLite".to_string();
        c.file = "/home/me/data/chinook.db".to_string();
        assert_eq!(c.endpoint(), "chinook.db");
        assert_eq!(c.file_label(), "/home/me/data/chinook.db");
    }

    /// `connections.json` is portable and the app runs on both platforms, so a
    /// path saved on Windows can be read on Linux — where `std::path` does not
    /// treat `\` as a separator and would hand back the whole path as the name.
    #[test]
    fn a_file_name_splits_on_either_platforms_separator() {
        let mut c = conn();
        c.db_type = "sqlite".to_string();
        for (path, name) in [
            (r"C:\Users\me\app.sqlite", "app.sqlite"),
            ("/var/lib/app.db", "app.db"),
            ("bare.db", "bare.db"),
            ("", ""),
            // A trailing separator has no name after it — not a panic.
            ("/var/lib/", ""),
        ] {
            c.file = path.to_string();
            assert_eq!(c.endpoint(), name, "{path}");
        }
    }

    /// Pointing a saved connection at another file reaches an entirely different
    /// set of tables — the same class of change as a repointed host, reached the
    /// same way (an edit in place, so the id doesn't move).
    #[test]
    fn repointing_a_sqlite_file_is_a_different_server() {
        let mut a = conn();
        a.db_type = "SQLite".to_string();
        a.file = "/data/one.db".to_string();
        let mut b = a.clone();
        assert!(a.targets_same_server(&b));
        b.file = "/data/two.db".to_string();
        assert!(
            !a.targets_same_server(&b),
            "another file is another database"
        );
        // And the presentational fields still don't count, as for any engine.
        let mut c = a.clone();
        c.name = "renamed".to_string();
        c.color = Some("#ff0000".to_string());
        assert!(a.targets_same_server(&c));
    }

    /// Every connection written before the field existed has no `file` key. It
    /// must load, not fail the whole file and take every saved connection with it.
    #[test]
    fn a_connection_saved_before_the_file_field_still_loads() {
        let json = r#"{
            "id": 3,
            "name": "old",
            "db_type": "MySQL",
            "host": "h",
            "port": 3306,
            "user": "u",
            "password": ""
        }"#;
        let c: Connection = serde_json::from_str(json).unwrap();
        assert_eq!(c.file, "");
        assert_eq!(c.id, 3);
    }

    fn conn() -> Connection {
        Connection {
            id: 1,
            name: "prod".to_string(),
            db_type: "MySQL".to_string(),
            host: "db.example.com".to_string(),
            port: 3307,
            user: "root".to_string(),
            password: "secret".to_string(),
            file: String::new(),
            ssh: SshTunnel::default(),
            tls: Tls::default(),
            color: None,
            prominent_color: false,
            read_only: false,
            environment: Environment::None,
            ai_data: None,
        }
    }

    /// A connection with every secret-bearing field filled — what a duplicate
    /// exists to avoid retyping.
    fn tunnelled() -> Connection {
        Connection {
            ssh: SshTunnel {
                enabled: true,
                host: "bastion.example.com".to_string(),
                port: 2222,
                user: "deploy".to_string(),
                password: "ssh-secret".to_string(),
                auth: SshAuth::KeyPair,
                key_path: "C:/keys/id_ed25519".to_string(),
                key_passphrase: "key-secret".to_string(),
            },
            ..conn()
        }
    }

    /// The whole point: the credentials come across. They live in the keyring,
    /// so a copy made by hand means retyping every one of them.
    #[test]
    fn a_duplicate_carries_the_credentials_and_the_whole_ssh_block() {
        let copy = tunnelled().duplicate(7, "copy".to_string(), None);
        assert_eq!(copy.host, "db.example.com");
        assert_eq!(copy.port, 3307);
        assert_eq!(copy.user, "root");
        assert_eq!(copy.password, "secret");
        assert_eq!(copy.db_type, "MySQL");
        assert_eq!(copy.ssh, tunnelled().ssh);
    }

    /// The struct-update form is supposed to carry a *newly added* field by
    /// construction. `file` is the first one added since, so this is what says the
    /// guarantee held — the failure it guards against (a copy silently pointing at
    /// no database) produces no compiler error in the field-by-field form.
    #[test]
    fn a_duplicate_carries_the_sqlite_file() {
        let src = Connection {
            db_type: "SQLite".to_string(),
            file: "/data/one.db".to_string(),
            ..conn()
        };
        let copy = src.duplicate(7, "copy".to_string(), None);
        assert_eq!(copy.file, "/data/one.db");
        assert_eq!(copy.endpoint(), "one.db");
    }

    /// …and only what identifies it *to the user* is replaced.
    #[test]
    fn a_duplicate_takes_the_new_identity_it_is_given() {
        let copy = conn().duplicate(7, "prod (copy)".to_string(), Some("#ff0000".into()));
        assert_eq!(copy.id, 7);
        assert_eq!(copy.name, "prod (copy)");
        assert_eq!(copy.color.as_deref(), Some("#ff0000"));
    }

    /// A guard-rail is not decoration: a copy of a read-only production
    /// connection that opened writable, or badged as something else, would be a
    /// worse trap than having no duplicate at all.
    #[test]
    fn a_duplicate_keeps_the_guard_rails() {
        let src = Connection {
            read_only: true,
            prominent_color: true,
            environment: Environment::Production,
            ..conn()
        };
        let copy = src.duplicate(7, "copy".to_string(), None);
        assert!(copy.read_only);
        assert!(copy.prominent_color);
        assert_eq!(copy.environment, Environment::Production);
    }

    /// The id is the **sole** component of the keyring account string, so two
    /// connections sharing one share all three secret slots. Past the maximum,
    /// not the count and not the first gap: an id freed by a delete stays free,
    /// or whatever still refers to it is adopted by the next connection created.
    #[test]
    fn a_new_connection_takes_an_id_past_every_one_in_use() {
        let with = |ids: &[u64]| -> Vec<Connection> {
            ids.iter()
                .map(|id| Connection { id: *id, ..conn() })
                .collect()
        };
        assert_eq!(Connection::next_id(&[]), 1, "never 0");
        assert_eq!(Connection::next_id(&with(&[1, 2, 3])), 4);
        // A gap in the middle is not reused.
        assert_eq!(Connection::next_id(&with(&[1, 3])), 4);
        // Nor is one at the end, after a delete.
        assert_eq!(Connection::next_id(&with(&[7])), 8);
        // Order-independent.
        assert_eq!(Connection::next_id(&with(&[9, 2, 5])), 10);
    }

    #[test]
    fn a_connection_targets_the_same_server_as_itself() {
        assert!(conn().targets_same_server(&conn()));
    }

    /// Presentation and guard-rails don't change what the server holds, so a
    /// rename (or a colour, or flipping read-only) must not count as a different
    /// target — the schema tree would throw away everything it has loaded.
    #[test]
    fn presentation_changes_are_the_same_server() {
        let mut edited = conn();
        edited.name = "production (eu)".into();
        edited.color = Some("#ff0000".into());
        edited.prominent_color = true;
        edited.read_only = true;
        edited.environment = Environment::Production;
        assert!(conn().targets_same_server(&edited));
    }

    /// Everything that decides which server the next query reaches. Each is
    /// checked on its own so a field dropped from the comparison fails here
    /// rather than silently showing one server's databases for another's.
    #[test]
    fn anything_that_moves_the_target_is_a_different_server() {
        fn moved(edit: impl Fn(&mut Connection)) -> bool {
            let mut edited = conn();
            edit(&mut edited);
            !conn().targets_same_server(&edited)
        }
        // `id` is **not** on this list, and used to head it. A connection's id
        // decides nothing about which server a query reaches; asserting it here
        // under this docstring is what presented the term as settled and let
        // the killed-session repair's honest call go dead. It is
        // `is_reload_of`'s, below.
        assert!(moved(|c| c.db_type = "PostgreSQL".into()), "engine");
        assert!(moved(|c| c.host = "other.example.com".into()), "host");
        assert!(moved(|c| c.port = 3306), "port");
        assert!(moved(|c| c.user = "reader".into()), "user");
        assert!(moved(|c| c.ssh.enabled = true), "ssh toggle");
        assert!(moved(|c| c.ssh.host = "bastion".into()), "ssh host");
        assert!(moved(|c| c.ssh.port = 2222), "ssh port");
        assert!(moved(|c| c.ssh.user = "deploy".into()), "ssh user");
    }

    /// **Two saved connections on one server target the same server**, which is
    /// the case the killed-session repair exists for: `local (app)` and `local
    /// (root)`, or two entries differing only in their default database. It
    /// answered `false`, because the predicate opened with `self.id ==
    /// other.id` and the repair's call site is reached *only* when the ids
    /// differ — dead code, and a pinned Manual tab left holding a dead socket
    /// with Commit and Rollback still offered.
    #[test]
    fn two_connections_on_one_server_target_the_same_server() {
        let mut b = conn();
        b.id = 2;
        b.name = "local (root)".into();
        assert!(conn().targets_same_server(&b));
    }

    /// The other half of the split: the schema tree keeps its rows only for a
    /// **reload of the same entry**, not for a switch to a different connection
    /// that happens to reach the same server — those rows are a different
    /// connection's and the toolbar, colour and read-only guard go with them.
    /// And not for the same entry repointed at a new host, which is an edit *in
    /// place* and keeps its id.
    #[test]
    fn a_reload_is_the_same_entry_still_pointing_where_it_did() {
        assert!(conn().is_reload_of(&conn()));

        let mut renamed = conn();
        renamed.name = "production (eu)".into();
        assert!(conn().is_reload_of(&renamed), "presentation moves nothing");

        let mut other_entry = conn();
        other_entry.id = 2;
        assert!(
            !conn().is_reload_of(&other_entry),
            "a switch, even to the same server"
        );

        let mut repointed = conn();
        repointed.host = "other.example.com".into();
        assert!(
            !conn().is_reload_of(&repointed),
            "the same entry, edited in place to point somewhere new"
        );
    }

    /// **A SQLite connection opens no tunnel, whatever the flag says.**
    ///
    /// The state is reachable and not exotic: change a saved MySQL connection's
    /// engine to SQLite, browse to a file, save. `ssh.enabled` comes across
    /// unchanged and the SQLite form renders no SSH block, so no control in the
    /// app can unset it. Asking the flag alone made every operation on a local
    /// file authenticate to a bastion with a stored credential, and fail
    /// outright when that host was down.
    #[test]
    fn a_sqlite_connection_never_uses_a_tunnel() {
        let mut c = conn();
        c.ssh.enabled = true;
        assert!(c.uses_tunnel(), "the premise: MySQL with SSH on");

        for label in ["SQLite", "sqlite", "SQLite3", " sqlite3 "] {
            let mut file = c.clone();
            file.db_type = label.into();
            assert!(
                !file.uses_tunnel(),
                "{label} has no server to tunnel to, and the form can't turn this off"
            );
        }
        // And the ordinary "no SSH configured" answer is unchanged on every
        // engine.
        for label in ["MySQL", "PostgreSQL", "SQLite"] {
            let mut off = c.clone();
            off.db_type = label.into();
            off.ssh.enabled = false;
            assert!(!off.uses_tunnel(), "{label}");
        }
    }

    /// **What this module's opening paragraph claims, asserted.** A SQLite
    /// connection has "no secret to keep … no tunnel to open", and switching a
    /// saved MySQL connection's engine to SQLite is what used to make that false:
    /// the form renders no SSH block, so nothing could unset what came across.
    #[test]
    fn saving_a_sqlite_connection_drops_the_server_side() {
        let mut switched = tunnelled();
        switched.db_type = "SQLite".into();
        switched.file = r"C:\data\app.db".into();
        let saved = switched.clone().sanitized();

        assert_eq!(saved.file, r"C:\data\app.db", "the target is kept");
        assert!(saved.host.is_empty());
        assert_eq!(saved.port, 0);
        assert!(saved.user.is_empty());
        assert!(saved.password.is_empty(), "nothing reaches the keyring");
        assert_eq!(saved.ssh, SshTunnel::default());
        assert!(!saved.uses_tunnel());
    }

    /// And a real server connection is returned exactly as it was — the whole
    /// point of the `sanitized` name is that it is a no-op on every other engine.
    #[test]
    fn sanitizing_a_server_connection_changes_nothing() {
        let c = tunnelled();
        assert_eq!(c.clone().sanitized(), c);
    }

    /// **The exclusion the doc spends a paragraph defending**, and which nothing
    /// asserted: a corrected password reaches the same databases, so counting it
    /// as a different server would blank the tree on every password fix.
    /// Reversing it was free.
    #[test]
    fn a_corrected_password_is_the_same_server() {
        let mut edited = conn();
        edited.password = "the right one".into();
        edited.ssh.password = "also corrected".into();
        edited.ssh.key_passphrase = "and this".into();
        assert!(conn().targets_same_server(&edited));
    }

    /// A **tripwire**, not a behaviour test. `SshTunnel` grows fields, and the
    /// comparison names four of them by hand — so a second hop, a bind address or
    /// a jump host would be excluded *silently*, and the tree would keep one
    /// server's databases while queries went to another. Writing the struct out
    /// in full means adding a field fails to compile here, at the one place that
    /// asks whether it moves the target.
    ///
    /// Update the literal **and** decide, for the new field: does it change which
    /// server the next query reaches? If so, add it to `targets_same_server` and
    /// to `anything_that_moves_the_target_is_a_different_server` above.
    #[test]
    fn every_ssh_field_has_been_judged() {
        let t = SshTunnel {
            enabled: true,
            host: "bastion".into(),
            port: 2222,
            user: "deploy".into(),
            // Credentials for the same endpoint: excluded, like the DB password.
            password: "p".into(),
            auth: SshAuth::KeyPair,
            key_path: "/k".into(),
            key_passphrase: "q".into(),
        };
        // `auth` and `key_path` are excluded deliberately: they are *how* the
        // same hop is authenticated, not a different hop.
        let mut a = conn();
        a.ssh = SshTunnel {
            auth: SshAuth::Password,
            key_path: String::new(),
            ..t.clone()
        };
        let mut b = conn();
        b.ssh = t;
        assert!(a.targets_same_server(&b));
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
        assert_eq!(c.environment, Environment::None);
    }

    #[test]
    fn environment_labels_and_all_cover_every_variant() {
        assert_eq!(Environment::ALL.len(), 6);
        assert_eq!(Environment::default(), Environment::None);
        assert_eq!(Environment::None.label(), "None");
        assert_eq!(Environment::Production.label(), "Production");
        // The unset environment shows no badge; every real one does.
        assert_eq!(Environment::None.badge_label(), None);
        assert_eq!(Environment::Production.badge_label(), Some("Production"));
        assert_eq!(Environment::Development.badge_label(), Some("Development"));
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

    #[test]
    fn every_postgres_alias_is_shown_under_one_name() {
        for label in ["PostgreSQL", "postgres", "PG", "  postgresql  "] {
            assert_eq!(engine_label(label), "PostgreSQL", "{label}");
        }
    }

    #[test]
    fn a_mysql_family_label_is_shown_as_saved() {
        // MariaDB is a different client and a different server; saying so is
        // more use than folding it into "MySQL".
        assert_eq!(engine_label("MySQL"), "MySQL");
        assert_eq!(engine_label("MariaDB"), "MariaDB");
        // Written before the field existed.
        assert_eq!(engine_label(""), "MySQL");
        assert_eq!(engine_label("   "), "MySQL");
    }

    #[test]
    fn postgres_is_recognised_by_any_of_its_labels() {
        for label in ["PostgreSQL", "postgres", "PG", "  postgresql  "] {
            assert!(is_postgres(label), "{label}");
            assert_eq!(default_port(label), 5432, "{label}");
        }
    }

    #[test]
    fn sqlite_is_recognised_by_any_of_its_labels() {
        for label in ["SQLite", "sqlite", "SQLITE3", "  sqlite  "] {
            assert!(is_sqlite(label), "{label}");
            assert!(!is_postgres(label), "{label}");
            assert_eq!(engine_label(label), "SQLite", "{label}");
            // Not a port but the absence of one — there is no server to reach.
            assert_eq!(default_port(label), 0, "{label}");
        }
    }

    /// The label predicates and the dialect must never disagree about an engine:
    /// `SqlDialect::from_db_type` used to re-spell the alias match in another
    /// module, so a label the connection list called Postgres could have parsed as
    /// MySQL. It delegates here now, and this is what says so.
    #[test]
    fn the_dialect_agrees_with_the_label_predicates() {
        use crate::intel::SqlDialect;
        for label in [
            "MySQL",
            "MariaDB",
            "",
            "PostgreSQL",
            "postgres",
            "PG",
            "SQLite",
            "sqlite3",
            "  SQLITE ",
            "something else",
        ] {
            let by_predicate = if is_postgres(label) {
                SqlDialect::Postgres
            } else if is_sqlite(label) {
                SqlDialect::Sqlite
            } else {
                SqlDialect::MySql
            };
            assert_eq!(SqlDialect::from_db_type(label), by_predicate, "{label}");
        }
    }

    /// Two labels for one engine are one engine — the question the connection
    /// form's Type picker asks to tell *its own* change apart from a load.
    #[test]
    fn labels_for_one_engine_name_the_same_engine() {
        for (a, b) in [
            ("MySQL", "MariaDB"),
            ("MariaDB", "mysql"),
            // Predates the field, and everything from that era was MySQL.
            ("", "MySQL"),
            ("something new", "MySQL"),
            ("PostgreSQL", "pg"),
            ("postgres", "  POSTGRESQL "),
            ("SQLite", "sqlite3"),
            ("  sqlite ", "SQLite"),
        ] {
            assert!(same_engine(a, b), "{a} vs {b}");
            assert!(same_engine(b, a), "{b} vs {a}");
        }
    }

    #[test]
    fn labels_for_different_engines_do_not() {
        for (a, b) in [
            ("MySQL", "PostgreSQL"),
            ("MariaDB", "SQLite"),
            ("PostgreSQL", "sqlite3"),
            ("", "pg"),
            ("SQLite", ""),
        ] {
            assert!(!same_engine(a, b), "{a} vs {b}");
            assert!(!same_engine(b, a), "{b} vs {a}");
        }
    }

    /// It has to answer through the same two predicates the rest of the app
    /// decides with, or a fourth engine would sort onto whichever side it fell.
    #[test]
    fn same_engine_agrees_with_the_label_predicates() {
        let labels = [
            "MySQL",
            "MariaDB",
            "",
            "PostgreSQL",
            "pg",
            "SQLite",
            "sqlite3",
            "something else",
        ];
        for a in labels {
            for b in labels {
                let by_predicate = is_postgres(a) == is_postgres(b) && is_sqlite(a) == is_sqlite(b);
                assert_eq!(same_engine(a, b), by_predicate, "{a} vs {b}");
            }
        }
    }

    #[test]
    fn anything_else_is_mysql_including_the_labels_that_predate_the_field() {
        for label in ["MySQL", "MariaDB", "", "something new"] {
            assert!(!is_postgres(label), "{label}");
            assert_eq!(default_port(label), 3306, "{label}");
        }
    }
}

#[cfg(test)]
mod tls_tests {
    use super::*;

    fn conn() -> Connection {
        Connection {
            id: 1,
            name: "prod".to_string(),
            db_type: "MySQL".to_string(),
            host: "db.example.com".to_string(),
            port: 3307,
            user: "root".to_string(),
            password: "secret".to_string(),
            file: String::new(),
            ssh: SshTunnel::default(),
            tls: Tls::default(),
            color: None,
            prominent_color: false,
            read_only: false,
            environment: Environment::None,
            ai_data: None,
        }
    }

    #[test]
    fn ssl_mode_labels_and_all_cover_every_variant() {
        assert_eq!(SslMode::ALL.len(), 5);
        assert_eq!(SslMode::Disable.label(), "Disable");
        assert_eq!(SslMode::Prefer.label(), "Prefer");
        assert_eq!(SslMode::Require.label(), "Require");
        assert_eq!(SslMode::VerifyCa.label(), "Verify CA");
        assert_eq!(SslMode::VerifyFull.label(), "Verify full");
        // Every mode says something about itself in the form; a blank one would
        // leave the riskiest choice the least explained.
        for m in SslMode::ALL {
            assert!(!m.description().is_empty(), "{m:?}");
        }
    }

    /// The four predicates are a *ladder*, and the whole point of splitting them
    /// is that each one is asked where it belongs. If a later mode were sorted
    /// onto the wrong rung — verifying a hostname without checking the chain it
    /// came from — the connection would report itself verified while trusting a
    /// certificate signed by nobody. Asserted over `ALL` so a new variant has to
    /// pass it rather than merely compile.
    #[test]
    fn the_verification_ladder_holds_for_every_mode() {
        for m in SslMode::ALL {
            if m.verifies_hostname() {
                assert!(
                    m.verifies_certificate(),
                    "{m:?} checks a name it can't trust"
                );
            }
            if m.verifies_certificate() {
                assert!(
                    m.requires_tls(),
                    "{m:?} verifies a session it would give up"
                );
            }
            if m.requires_tls() {
                assert!(m.negotiates_tls(), "{m:?} requires TLS it never offers");
            }
        }
    }

    #[test]
    fn prefer_is_the_default_and_never_mandates_tls() {
        assert_eq!(SslMode::default(), SslMode::Prefer);
        assert_eq!(Tls::default().mode, SslMode::Prefer);
        // The property that makes Prefer safe as the default for connections
        // saved before this setting existed: it can encrypt, but it can never
        // refuse a server that plaintext would have reached.
        assert!(SslMode::Prefer.negotiates_tls());
        assert!(!SslMode::Prefer.requires_tls());
        assert!(!SslMode::Disable.negotiates_tls());
    }

    #[test]
    fn a_connection_saved_before_tls_existed_reads_as_prefer() {
        let json = r#"{
            "id": 7,
            "name": "legacy",
            "host": "127.0.0.1",
            "port": 3306,
            "user": "app",
            "password": ""
        }"#;
        let c: Connection = serde_json::from_str(json).unwrap();
        assert_eq!(c.tls, Tls::default());
        assert_eq!(c.tls.mode, SslMode::Prefer);
    }

    /// A mode written by a newer build must degrade to the default rather than
    /// fail the deserialize — which, because `connections.json` is one document,
    /// would take out *every* connection and not just this field.
    #[test]
    fn an_unknown_ssl_mode_degrades_instead_of_failing_the_file() {
        let json = r#"{"mode":"VerifyFullPlusSomethingNew","ca_path":"/ca.crt"}"#;
        let t: Tls = serde_json::from_str(json).unwrap();
        assert_eq!(t.mode, SslMode::default());
        // …and the rest of the block still parses.
        assert_eq!(t.ca_path, "/ca.crt");
    }

    #[test]
    fn a_tls_block_round_trips_through_json() {
        let mut c = conn();
        c.tls = Tls {
            mode: SslMode::VerifyFull,
            ca_path: "/etc/ca.crt".into(),
            client_cert_path: "/etc/client.crt".into(),
            client_key_path: "/etc/client.key".into(),
            client_key_passphrase: "pw".into(),
        };
        let back: Connection = serde_json::from_str(&serde_json::to_string(&c).unwrap()).unwrap();
        assert_eq!(back.tls, c.tls);
    }

    /// SQLite has no server, so it has no transport to secure. The same trap as
    /// the SSH block: the engine picker is editable in place and the SQLite form
    /// renders no TLS section, so a connection switched over from MySQL would
    /// keep a CA path and a `verify-full` with no control anywhere that could
    /// unset either.
    #[test]
    fn a_sqlite_connection_is_saved_without_tls_settings() {
        let mut c = conn();
        c.db_type = "SQLite".into();
        c.tls = Tls {
            mode: SslMode::VerifyFull,
            ca_path: "/etc/ca.crt".into(),
            ..Tls::default()
        };
        assert_eq!(c.sanitized().tls, Tls::default());
    }

    #[test]
    fn uses_tls_is_false_for_sqlite_however_the_mode_reads() {
        let mut c = conn();
        c.tls.mode = SslMode::VerifyFull;
        assert!(c.uses_tls());

        c.db_type = "SQLite".into();
        assert!(!c.uses_tls());

        let mut plain = conn();
        plain.tls.mode = SslMode::Disable;
        assert!(!plain.uses_tls());
    }

    /// Half a client identity is not a weaker identity, it is a broken connect:
    /// a certificate with no key cannot answer the server's challenge, so the
    /// pair is asked for together or not at all.
    #[test]
    fn a_client_certificate_needs_both_halves() {
        let mut t = Tls {
            mode: SslMode::Require,
            client_cert_path: "/c.crt".into(),
            ..Tls::default()
        };
        assert!(!t.uses_client_cert(), "a certificate with no key");
        t.client_key_path = "/c.key".into();
        assert!(t.uses_client_cert());
        t.client_cert_path = String::new();
        assert!(!t.uses_client_cert(), "a key with no certificate");
    }

    /// A client certificate is an offer made during the handshake, so it is
    /// meaningless on a connection that never handshakes.
    #[test]
    fn a_disabled_connection_offers_no_client_certificate() {
        let t = Tls {
            mode: SslMode::Disable,
            client_cert_path: "/c.crt".into(),
            client_key_path: "/c.key".into(),
            ..Tls::default()
        };
        assert!(!t.uses_client_cert());
    }

    /// An empty CA path under a verifying mode is not "verify nothing" — it is
    /// "verify against the bundled public roots", which is what a hosted provider
    /// with a publicly-signed certificate needs. Reading it as a missing file
    /// instead would make `verify-full` unusable against exactly the servers it
    /// exists for.
    #[test]
    fn the_ca_file_is_only_consulted_by_a_verifying_mode() {
        let mut t = Tls {
            mode: SslMode::Require,
            ca_path: "/etc/ca.crt".into(),
            ..Tls::default()
        };
        assert_eq!(t.ca_file(), None, "require verifies nothing to verify with");

        t.mode = SslMode::VerifyCa;
        assert_eq!(t.ca_file(), Some("/etc/ca.crt"));

        t.ca_path = String::new();
        assert_eq!(t.ca_file(), None, "empty means the bundled public roots");
    }

    /// The plan is what every driver is actually configured from, so this is the
    /// table the whole feature reduces to. Written out mode by mode rather than
    /// derived from the predicates, because a plan computed from the same
    /// expression it is checked against would agree with itself no matter what
    /// either said.
    #[test]
    fn each_mode_plans_the_handshake_it_promises() {
        let of = |mode| {
            Tls {
                mode,
                ..Tls::default()
            }
            .plan()
        };

        assert!(of(SslMode::Disable).is_none(), "disable never handshakes");

        let prefer = of(SslMode::Prefer).expect("prefer handshakes");
        assert!(prefer.fallback_to_plaintext);
        assert!(prefer.accept_invalid_certs);
        assert!(prefer.skip_hostname_check);

        let require = of(SslMode::Require).expect("require handshakes");
        assert!(!require.fallback_to_plaintext, "require may not fall back");
        assert!(require.accept_invalid_certs);
        assert!(require.skip_hostname_check);

        let ca = of(SslMode::VerifyCa).expect("verify-ca handshakes");
        assert!(!ca.fallback_to_plaintext);
        assert!(!ca.accept_invalid_certs, "the chain is checked");
        assert!(ca.skip_hostname_check, "but the name is not");

        let full = of(SslMode::VerifyFull).expect("verify-full handshakes");
        assert!(!full.fallback_to_plaintext);
        assert!(!full.accept_invalid_certs);
        assert!(!full.skip_hostname_check, "the name is checked too");
    }

    /// Only [`SslMode::Prefer`] may quietly end up unencrypted. Stated as its own
    /// property because it is the one that would make the whole setting a lie:
    /// a `require` that fell back looks identical to a `require` that worked.
    #[test]
    fn only_prefer_may_end_up_in_plaintext() {
        for m in SslMode::ALL {
            let planned_fallback = Tls {
                mode: m,
                ..Tls::default()
            }
            .plan()
            .is_some_and(|p| p.fallback_to_plaintext);
            assert_eq!(planned_fallback, m == SslMode::Prefer, "{m:?}");
        }
    }

    #[test]
    fn the_plan_carries_the_files_the_mode_actually_uses() {
        let mut t = Tls {
            mode: SslMode::VerifyFull,
            ca_path: "/etc/ca.crt".into(),
            client_cert_path: "/c.crt".into(),
            client_key_path: "/c.key".into(),
            client_key_passphrase: "pw".into(),
        };
        let p = t.plan().expect("handshakes");
        assert_eq!(p.root_ca.as_deref(), Some("/etc/ca.crt"));
        assert_eq!(
            p.client_identity,
            Some(("/c.crt".to_string(), "/c.key".to_string()))
        );

        // A non-verifying mode has nothing to verify the CA against, so the path
        // is not carried — but the client identity still is, since offering one
        // is unrelated to checking theirs.
        t.mode = SslMode::Require;
        let p = t.plan().expect("handshakes");
        assert_eq!(p.root_ca, None);
        assert!(p.client_identity.is_some());
    }

    /// A connection is planned through the engine, not the mode: SQLite is a
    /// local file, and a `verify-full` left on it by the engine picker must not
    /// reach a driver that would then look for a CA file.
    #[test]
    fn a_sqlite_connection_plans_no_handshake() {
        let mut c = conn();
        c.tls.mode = SslMode::VerifyFull;
        assert!(c.tls_plan().is_some());

        c.db_type = "SQLite".into();
        assert!(c.tls_plan().is_none());
    }

    /// A **tripwire**, not a behaviour test. `Tls` grows fields, and every one of
    /// them so far is *how* the same server is reached rather than *which*
    /// server — so none belongs in `targets_same_server`, and the schema tree
    /// must not blank itself when a CA path is corrected.
    ///
    /// Update the literal **and** decide, for the new field: does it change which
    /// server the next query reaches? A field that does belongs in
    /// `targets_same_server` alongside host and port.
    #[test]
    fn every_tls_field_has_been_judged() {
        let t = Tls {
            mode: SslMode::VerifyFull,
            ca_path: "/etc/ca.crt".into(),
            client_cert_path: "/etc/client.crt".into(),
            client_key_path: "/etc/client.key".into(),
            client_key_passphrase: "pw".into(),
        };
        let mut edited = conn();
        edited.tls = t;
        assert!(conn().targets_same_server(&edited));
    }

    /// The credentials are the point of a duplicate, and a CA path plus a client
    /// key is as much retyping as an SSH block. Carried by the struct update in
    /// `duplicate`, so this guards that it stays a struct update.
    #[test]
    fn a_duplicate_carries_the_tls_settings() {
        let mut c = conn();
        c.tls = Tls {
            mode: SslMode::VerifyFull,
            ca_path: "/etc/ca.crt".into(),
            client_cert_path: "/etc/client.crt".into(),
            client_key_path: "/etc/client.key".into(),
            client_key_passphrase: "pw".into(),
        };
        let copy = c.duplicate(9, "copy".into(), None);
        assert_eq!(copy.tls, c.tls);
    }
}
