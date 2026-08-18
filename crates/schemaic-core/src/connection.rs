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
    /// string (`conn.{id}.password` / `.ssh_password` / `.ssh_passphrase`), so
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
    pub fn targets_same_server(&self, other: &Connection) -> bool {
        self.id == other.id
            && self.db_type == other.db_type
            && self.file == other.file
            && self.host == other.host
            && self.port == other.port
            && self.user == other.user
            && self.ssh.enabled == other.ssh.enabled
            && self.ssh.host == other.ssh.host
            && self.ssh.port == other.ssh.port
            && self.ssh.user == other.ssh.user
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
            color: None,
            prominent_color: false,
            read_only: false,
            environment: Environment::None,
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
            color: None,
            prominent_color: false,
            read_only: false,
            environment: Environment::None,
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
        assert!(moved(|c| c.id = 2), "id");
        assert!(moved(|c| c.db_type = "PostgreSQL".into()), "engine");
        assert!(moved(|c| c.host = "other.example.com".into()), "host");
        assert!(moved(|c| c.port = 3306), "port");
        assert!(moved(|c| c.user = "reader".into()), "user");
        assert!(moved(|c| c.ssh.enabled = true), "ssh toggle");
        assert!(moved(|c| c.ssh.host = "bastion".into()), "ssh host");
        assert!(moved(|c| c.ssh.port = 2222), "ssh port");
        assert!(moved(|c| c.ssh.user = "deploy".into()), "ssh user");
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

    #[test]
    fn anything_else_is_mysql_including_the_labels_that_predate_the_field() {
        for label in ["MySQL", "MariaDB", "", "something new"] {
            assert!(!is_postgres(label), "{label}");
            assert_eq!(default_port(label), 3306, "{label}");
        }
    }
}
