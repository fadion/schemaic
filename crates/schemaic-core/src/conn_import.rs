//! Reading connections **out of other tools**: a pasted URL/DSN, DBeaver's
//! `data-sources.json`, DataGrip's `dataSources.xml`, and the three plain-text
//! files the command-line clients read — `~/.my.cnf`, `~/.pgpass` and
//! `~/.pg_service.conf`.
//!
//! Arriving from another client used to mean re-typing every server by hand,
//! which is the actual switching cost. Everything here is pure over *text*: the
//! app finds the files and reads them, this decides what they mean. That split
//! is what makes the whole surface unit-testable, and it is why the one entry
//! point ([`scan`]) takes [`SourceFile`]s rather than paths.
//!
//! **An import is a proposal, never a write.** Every parser answers with
//! [`Imported`] values carrying `id: 0` and a *suggested* name; the app assigns
//! the real id (`Connection::next_id`) and uniquifies the name at the moment the
//! user accepts a row. Nothing here touches the keyring, and nothing here
//! decides that a connection is worth keeping.
//!
//! **Only decide what can be decided** — the rule [`crate::import`] and
//! [`crate::intel`] already work by. A source that does not carry a password
//! yields a connection without one plus an [`ImportNote::NoPassword`], not a
//! guess; a driver this app has no engine for is [`Skipped`] by name rather than
//! bent onto the nearest engine, because a MySQL connection silently pointed at
//! an Oracle server is a worse answer than an honest omission.
//!
//! Where the passwords actually are, since it decides what each source is worth:
//!
//! | Source | Server | User | Password |
//! |---|---|---|---|
//! | URL / DSN | yes | yes | when written into the URL |
//! | DBeaver | yes | usually | no — encrypted in `credentials-config.json` |
//! | DataGrip | yes | yes | no — in the OS credential store |
//! | `.my.cnf` | yes | yes | **yes**, plaintext |
//! | `.pgpass` | yes | yes | **yes**, plaintext |
//! | `.pg_service.conf` | yes | yes | when written there |
//!
//! That table is the reason [`fill_missing_passwords`] exists: `.pgpass` is the
//! one file on a PostgreSQL user's machine that can complete the twelve
//! server-only connections DataGrip just handed over, and matching them up is
//! four string comparisons libpq already specifies.

use crate::connection::{
    Connection, Environment, SshAuth, SshTunnel, SslMode, Tls, default_port, is_postgres,
    is_sqlite, same_engine,
};
// Every source here is a file somebody else's tool wrote, on a platform where
// the editors write a BOM. Each parser strips one at its own door, so calling a
// parser directly is as safe as going through `scan`.
use crate::text::strip_bom;

/// The engine labels this module writes into `Connection::db_type`.
///
/// Spelled as `crate::connection::engine_label` would show them, so an imported
/// connection is indistinguishable from a hand-typed one in the switcher. They
/// are *labels*, not capability answers — read them back through `is_postgres` /
/// `is_sqlite` like every other `db_type`.
const MYSQL: &str = "MySQL";
const MARIADB: &str = "MariaDB";
const POSTGRES: &str = "PostgreSQL";
const SQLITE: &str = "SQLite";

/// Which tool a connection was read out of.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ImportSource {
    /// A URL or DSN the user pasted.
    Url,
    /// DBeaver's `data-sources.json`.
    DBeaver,
    /// DataGrip's (or any JetBrains IDE's) `dataSources.xml`.
    DataGrip,
    /// `~/.my.cnf` — the MySQL/MariaDB command-line client's defaults file.
    MyCnf,
    /// `~/.pgpass` — libpq's password file.
    Pgpass,
    /// `~/.pg_service.conf` — libpq's named-service file.
    PgService,
}

impl ImportSource {
    /// How the source is named on screen, in the row that came from it.
    pub fn label(self) -> &'static str {
        match self {
            ImportSource::Url => "URL",
            ImportSource::DBeaver => "DBeaver",
            ImportSource::DataGrip => "DataGrip",
            ImportSource::MyCnf => ".my.cnf",
            ImportSource::Pgpass => ".pgpass",
            ImportSource::PgService => ".pg_service.conf",
        }
    }
}

/// What the user needs to know about one imported row *before* accepting it.
///
/// Notes are advisory — none of them stops an import. They exist because the
/// failure mode of this feature is a list of twelve plausible rows, three of
/// which will not connect for a reason the source file already knew.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ImportNote {
    /// The source carries no password for this connection. Expected for DBeaver
    /// and DataGrip, which keep them elsewhere.
    NoPassword,
    /// A saved connection already points at this same endpoint. Left in the list
    /// rather than dropped — the saved one may be stale — but not pre-selected.
    AlreadySaved,
    /// The path still contains a macro the source tool expands for itself
    /// (`$PROJECT_DIR$`, `$USER_HOME$`). It will not resolve here as written.
    UnexpandedPath,
}

impl ImportNote {
    /// The one line shown beside the row.
    pub fn label(self) -> &'static str {
        match self {
            ImportNote::NoPassword => "No password in the source",
            ImportNote::AlreadySaved => "Already saved",
            ImportNote::UnexpandedPath => "Path contains an unexpanded macro",
        }
    }
}

/// One connection a source offered, with everything the review list needs.
#[derive(Clone, PartialEq, Debug)]
pub struct Imported {
    /// The connection as parsed. `id` is **0** — a placeholder, not an identity;
    /// the app assigns the real one on accept.
    pub connection: Connection,
    /// Which tool it came from.
    pub source: ImportSource,
    /// Where exactly — a file path, or empty for a pasted URL. Shown so two rows
    /// with the same name from two DataGrip projects can be told apart.
    pub origin: String,
    /// Advisory flags; see [`ImportNote`].
    pub notes: Vec<ImportNote>,
}

impl Imported {
    /// Does this row carry `note`?
    pub fn has(&self, note: ImportNote) -> bool {
        self.notes.contains(&note)
    }

    /// Should the review list tick this row by default?
    ///
    /// Everything except a duplicate of something already saved. A missing
    /// password is not a reason to skip a row — the user is about to type it —
    /// but re-adding a server they already have is the one outcome nobody wants
    /// by accident.
    pub fn preselected(&self) -> bool {
        !self.has(ImportNote::AlreadySaved)
    }

    fn note(&mut self, note: ImportNote) {
        if !self.notes.contains(&note) {
            self.notes.push(note);
        }
    }
}

/// Why an entry a source *did* contain is not on offer.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum SkipReason {
    /// The driver names an engine this app has no support for (Oracle, SQL
    /// Server, Snowflake…). Carries the driver/provider string as written.
    UnsupportedEngine(String),
    /// Recognisably a connection, but with nothing to connect to — no host, or a
    /// `.pgpass` wildcard that names every server and therefore none.
    NoServer,
    /// The file itself did not parse. Carries the parser's complaint.
    Unreadable(String),
}

impl SkipReason {
    /// The explanation shown after the entry's name.
    pub fn message(&self) -> String {
        match self {
            SkipReason::UnsupportedEngine(d) => format!("unsupported engine ({d})"),
            SkipReason::NoServer => "no server to connect to".to_string(),
            SkipReason::Unreadable(e) => format!("could not be read ({e})"),
        }
    }
}

/// An entry that was found and is **not** being offered, so the count in the
/// modal is honest about what the file held.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Skipped {
    /// The entry's name, or the file's path when the file as a whole failed.
    pub name: String,
    /// Why it isn't in the list.
    pub reason: SkipReason,
}

/// What one parse — or a whole [`scan`] — produced.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct ImportScan {
    /// Connections on offer, in the order the sources listed them.
    pub found: Vec<Imported>,
    /// Entries deliberately left out; see [`Skipped`].
    pub skipped: Vec<Skipped>,
}

impl ImportScan {
    /// Fold another scan's results into this one, preserving order.
    pub fn merge(&mut self, other: ImportScan) {
        self.found.extend(other.found);
        self.skipped.extend(other.skipped);
    }

    fn skip(&mut self, name: impl Into<String>, reason: SkipReason) {
        self.skipped.push(Skipped {
            name: name.into(),
            reason,
        });
    }
}

/// One source file, already read. The app does the finding and the reading; this
/// module does everything after.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SourceFile {
    /// Which parser to run.
    pub source: ImportSource,
    /// Where it came from, for the row's subtitle.
    pub path: String,
    /// The file's contents.
    pub text: String,
}

/// Parse every source file, complete what can be completed, and mark what is
/// already saved.
///
/// **The one entry point.** The order matters and is the whole reason this is a
/// function rather than a loop at the call site: `.pgpass` passwords are applied
/// *after* every source has been parsed (so they can complete a DataGrip row),
/// duplicates are collapsed *after* that (so the completed row is the one that
/// survives), and only then is the result compared against what the user
/// already has.
pub fn scan(files: &[SourceFile], existing: &[Connection]) -> ImportScan {
    let mut out = ImportScan::default();
    let mut pgpass: Vec<PgpassEntry> = Vec::new();

    for f in files {
        if f.source == ImportSource::Pgpass {
            pgpass.extend(pgpass_entries(&f.text));
        }
        let mut one = match f.source {
            ImportSource::Url => parse_url_scan(&f.text),
            ImportSource::DBeaver => parse_dbeaver(&f.text),
            ImportSource::DataGrip => parse_datagrip(&f.text),
            ImportSource::MyCnf => parse_my_cnf(&f.text),
            ImportSource::Pgpass => parse_pgpass(&f.text),
            ImportSource::PgService => parse_pg_service(&f.text),
        };
        for imp in &mut one.found {
            imp.origin = f.path.clone();
        }
        // A parser that could not read the file at all has no entry name to
        // report; the file's path is what the user needs to see instead. Every
        // such entry, not just the first — "the first one is the only one that
        // can be unnamed" is a fact about today's parsers, and the modal would
        // render a later one as a bare " (could not be read …)".
        for entry in &mut one.skipped {
            if entry.name.is_empty() {
                entry.name = f.path.clone();
            }
        }
        out.merge(one);
    }

    fill_missing_passwords(&mut out.found, &pgpass);
    out.found = dedupe(out.found);
    mark_existing(&mut out.found, existing);
    for imp in &mut out.found {
        if imp.connection.password.is_empty() && needs_password(&imp.connection) {
            imp.note(ImportNote::NoPassword);
        }
    }
    out
}

/// Would a missing password actually stop this connection?
///
/// SQLite has no server and therefore no credentials, and a connection with no
/// user named isn't authenticating as anybody either — flagging those would put
/// a warning on every local file the user imports.
fn needs_password(c: &Connection) -> bool {
    !is_sqlite(&c.db_type) && !c.user.trim().is_empty()
}

// ---------------------------------------------------------------------------
// URL / DSN
// ---------------------------------------------------------------------------

/// Why a pasted string is not a connection URL.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum UrlError {
    /// Nothing (or only whitespace) was pasted.
    Empty,
    /// No `scheme:` at the front — most likely a bare `host:port`.
    NoScheme,
    /// A scheme, but not one of ours. Carries it, since "postgersql" is the
    /// common case and seeing it back is the fix.
    UnknownScheme(String),
    /// A server URL with no host in it.
    NoHost,
    /// The port is not a number. Carries what was there.
    BadPort(String),
    /// A `sqlite:` URL with no path after the scheme.
    NoPath,
}

impl UrlError {
    /// The message shown under the paste field.
    pub fn message(&self) -> String {
        match self {
            UrlError::Empty => "Paste a connection URL.".to_string(),
            UrlError::NoScheme => {
                "No scheme — a URL starts with mysql://, postgresql:// or sqlite:.".to_string()
            }
            UrlError::UnknownScheme(s) => {
                format!("Unknown scheme \"{s}\" — expected mysql, mariadb, postgresql or sqlite.")
            }
            UrlError::NoHost => "No host in the URL.".to_string(),
            UrlError::BadPort(p) => format!("\"{p}\" is not a port number."),
            UrlError::NoPath => "No database file after sqlite:.".to_string(),
        }
    }
}

/// Parse one connection URL / DSN.
///
/// Accepts more than a strict URL parser would, because the strings people
/// actually hold are not strict URLs: a `DATABASE_URL=…` line lifted out of a
/// `.env` (with or without `export` and quotes), a `jdbc:` wrapper, libpq's
/// comma-separated host list, and a bracketed IPv6 literal. Query parameters are
/// read for the credentials and TLS settings JDBC puts there rather than in the
/// authority — `?user=&password=` is how a JDBC URL usually carries them at all.
///
/// The connection comes back with `id: 0` and a suggested name; see the module
/// docs.
pub fn parse_url(input: &str) -> Result<Connection, UrlError> {
    let raw = strip_env_assignment(strip_bom(input).trim());
    if raw.is_empty() {
        return Err(UrlError::Empty);
    }
    // `jdbc:` is a wrapper around the URL the driver itself reads.
    let raw = strip_prefix_ci(raw, "jdbc:").unwrap_or(raw);
    let (scheme, rest) = split_scheme(raw).ok_or(UrlError::NoScheme)?;
    let engine = match engine_for_scheme(scheme) {
        Some(e) => e,
        // `localhost:3306` splits at a colon exactly as a URL does, and calling
        // its host an unknown *engine* is a message that sends the reader
        // looking in the wrong place. Nothing but a port follows a host.
        None if rest.bytes().all(|b| b.is_ascii_digit()) => return Err(UrlError::NoScheme),
        None => return Err(UrlError::UnknownScheme(scheme.to_string())),
    };
    let mut c = if is_sqlite(engine) {
        parse_sqlite_url(rest)?
    } else {
        parse_server_url(engine, rest)?
    };
    c.name = suggest_name(&c);
    Ok(c)
}

/// [`parse_url`] as a scan, so the paste field and the file sources answer in
/// one shape.
fn parse_url_scan(text: &str) -> ImportScan {
    let mut out = ImportScan::default();
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        match parse_url(line) {
            Ok(c) => out.found.push(Imported {
                connection: c,
                source: ImportSource::Url,
                origin: String::new(),
                notes: Vec::new(),
            }),
            Err(e) => out.skip(line.trim(), SkipReason::Unreadable(e.message())),
        }
    }
    out
}

fn parse_sqlite_url(rest: &str) -> Result<Connection, UrlError> {
    // Strip the query (`?mode=ro`) and fragment; neither names the file.
    let path = rest.split(['?', '#']).next().unwrap_or("");
    // `sqlite:///abs/path` — the authority is empty, so two slashes are the
    // URL's and the third is the path's. `sqlite://relative.db` has no third.
    let path = path.strip_prefix("//").unwrap_or(path);
    // `sqlite:///C:/db.sqlite`: what's left is `/C:/db.sqlite`, and the leading
    // slash belongs to the URL, not to Windows.
    let path = match path.strip_prefix('/') {
        Some(tail) if looks_like_drive_path(tail) => tail,
        _ => path,
    };
    let path = percent_decode(path);
    if path.trim().is_empty() {
        return Err(UrlError::NoPath);
    }
    let mut c = blank(SQLITE);
    c.file = path;
    Ok(c)
}

fn parse_server_url(engine: &str, rest: &str) -> Result<Connection, UrlError> {
    let rest = rest.strip_prefix("//").unwrap_or(rest);
    let rest = rest.split('#').next().unwrap_or("");
    let (before_q, query) = match rest.split_once('?') {
        Some((a, b)) => (a, b),
        None => (rest, ""),
    };
    let (authority, path) = match before_q.split_once('/') {
        Some((a, b)) => (a, b),
        None => (before_q, ""),
    };

    let (userinfo, hostport) = match authority.rfind('@') {
        Some(i) => (Some(&authority[..i]), &authority[i + 1..]),
        None => (None, authority),
    };
    // libpq accepts `host1:5432,host2:5432`; a connection points at one server.
    let hostport = hostport.split(',').next().unwrap_or("");
    let (host, port) = split_host_port(hostport)?;
    if host.is_empty() {
        return Err(UrlError::NoHost);
    }

    let mut c = blank(engine);
    c.host = host;
    c.port = match port {
        Some(p) => p,
        None => default_port(engine),
    };
    if let Some(ui) = userinfo {
        let (u, p) = match ui.split_once(':') {
            Some((u, p)) => (u, p),
            None => (ui, ""),
        };
        c.user = percent_decode(u);
        c.password = percent_decode(p);
    }
    let db = path.split('/').next().unwrap_or("");
    c.database = percent_decode(db);
    apply_params(&mut c, &parse_query(query));
    Ok(c)
}

/// `host`, `host:5432`, `[::1]`, `[::1]:5432`.
fn split_host_port(s: &str) -> Result<(String, Option<u16>), UrlError> {
    if let Some(tail) = s.strip_prefix('[') {
        let (host, after) = tail.split_once(']').ok_or(UrlError::NoHost)?;
        let port = match after.strip_prefix(':') {
            Some(p) if !p.is_empty() => Some(parse_port(p)?),
            _ => None,
        };
        return Ok((host.to_string(), port));
    }
    match s.rsplit_once(':') {
        // `host:` — a colon with nothing after it is not a port, and parsing it
        // as one would refuse a URL that names a perfectly good server.
        Some((h, "")) => Ok((h.to_string(), None)),
        Some((h, p)) => Ok((h.to_string(), Some(parse_port(p)?))),
        None => Ok((s.to_string(), None)),
    }
}

fn parse_port(s: &str) -> Result<u16, UrlError> {
    s.parse::<u16>()
        .map_err(|_| UrlError::BadPort(s.to_string()))
}

/// `DATABASE_URL="postgres://…"`, `export DATABASE_URL=postgres://…`, or the
/// URL on its own — the three shapes a URL is copied in.
///
/// Only a *leading* `NAME=` is stripped, and only when the name looks like an
/// environment variable: a `?password=x` inside the URL must survive.
fn strip_env_assignment(s: &str) -> &str {
    let s = strip_prefix_ci(s, "export ").unwrap_or(s).trim_start();
    let s = match s.split_once('=') {
        Some((name, rest))
            if !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') =>
        {
            rest
        }
        _ => s,
    };
    let s = s.trim();
    // A shell-quoted value.
    for q in ['"', '\''] {
        if let Some(inner) = s.strip_prefix(q).and_then(|t| t.strip_suffix(q)) {
            return inner;
        }
    }
    s
}

fn strip_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    (s.len() >= prefix.len() && s[..prefix.len()].eq_ignore_ascii_case(prefix))
        .then(|| &s[prefix.len()..])
}

/// `scheme:rest`, where the scheme is a plausible URL scheme rather than the
/// `host` half of a bare `host:port`.
fn split_scheme(s: &str) -> Option<(&str, &str)> {
    let (scheme, rest) = s.split_once(':')?;
    let ok = !scheme.is_empty()
        && scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'-' || b == b'.');
    ok.then_some((scheme, rest))
}

/// Which engine a URL scheme names, or `None` for one we don't run.
///
/// The `+`-suffixed forms are SQLAlchemy's driver selectors
/// (`postgresql+psycopg2`), which name the same server.
fn engine_for_scheme(scheme: &str) -> Option<&'static str> {
    let base = scheme.split('+').next().unwrap_or("").to_ascii_lowercase();
    match base.as_str() {
        "mysql" => Some(MYSQL),
        "mariadb" => Some(MARIADB),
        "postgres" | "postgresql" | "pgsql" | "psql" => Some(POSTGRES),
        // `file:` is SQLite's own URI scheme, and the context here is database
        // URLs — nothing else in this app would paste one.
        "sqlite" | "sqlite3" | "file" => Some(SQLITE),
        _ => None,
    }
}

fn parse_query(q: &str) -> Vec<(String, String)> {
    q.split(['&', ';'])
        .filter(|p| !p.trim().is_empty())
        .map(|p| match p.split_once('=') {
            Some((k, v)) => (percent_decode(k), percent_decode(v)),
            None => (percent_decode(p), String::new()),
        })
        .collect()
}

/// Fold a URL's query parameters into the connection.
///
/// Explicit authority components win: `mysql://u@h/?user=other` is a URL whose
/// author meant `u`, and the parameter is the fallback JDBC needs because its
/// URLs mostly have no userinfo at all.
fn apply_params(c: &mut Connection, params: &[(String, String)]) {
    for (k, v) in params {
        if v.is_empty() {
            continue;
        }
        match normalize_key(k).as_str() {
            "user" | "username" | "uid" => set_if_empty(&mut c.user, v),
            "password" | "pwd" | "pass" => set_if_empty(&mut c.password, v),
            "database" | "dbname" | "db" => set_if_empty(&mut c.database, v),
            "sslmode" => {
                if let Some(m) = sslmode_from_str(v) {
                    c.tls.mode = m;
                }
            }
            // MySQL's boolean spellings. Only ever *raises* the mode, so an
            // explicit `sslmode` alongside them is not undone by ordering.
            "ssl" | "usessl" | "requiressl" => {
                if truthy(v) && !c.tls.mode.negotiates_tls() {
                    c.tls.mode = SslMode::Require;
                }
            }
            "sslrootcert" | "sslca" | "trustcertificatekeystoreurl" => {
                set_if_empty(&mut c.tls.ca_path, v)
            }
            "sslcert" => set_if_empty(&mut c.tls.client_cert_path, v),
            "sslkey" => set_if_empty(&mut c.tls.client_key_path, v),
            _ => {}
        }
    }
}

/// Parameter and INI keys, compared without the punctuation the various tools
/// disagree about: `ssl-mode`, `ssl_mode` and `sslMode` are one key.
fn normalize_key(k: &str) -> String {
    k.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

fn set_if_empty(dst: &mut String, v: &str) {
    if dst.is_empty() {
        *dst = v.to_string();
    }
}

fn truthy(v: &str) -> bool {
    matches!(
        v.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on" | "required" | "require"
    )
}

/// libpq's `sslmode` values **and** MySQL's `ssl-mode` values, which name the
/// same ladder with different words.
///
/// Unrecognised leaves the mode alone rather than defaulting: a mode we do not
/// know is not evidence that TLS is off.
fn sslmode_from_str(v: &str) -> Option<SslMode> {
    match normalize_key(v).as_str() {
        "disable" | "disabled" | "allow" | "off" | "false" => Some(SslMode::Disable),
        "prefer" | "preferred" => Some(SslMode::Prefer),
        "require" | "required" => Some(SslMode::Require),
        "verifyca" => Some(SslMode::VerifyCa),
        "verifyfull" | "verifyidentity" => Some(SslMode::VerifyFull),
        _ => None,
    }
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let (Some(hi), Some(lo)) = (
                (b[i + 1] as char).to_digit(16),
                (b[i + 2] as char).to_digit(16),
            )
        {
            out.push((hi * 16 + lo) as u8);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `C:/db.sqlite`, `d:\db.sqlite` — a Windows path that a URL's leading slash
/// has been prepended to.
fn looks_like_drive_path(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':'
}

// ---------------------------------------------------------------------------
// DBeaver
// ---------------------------------------------------------------------------

/// Parse a DBeaver `data-sources.json`.
///
/// Read through `serde_json::Value` rather than a typed struct on purpose: the
/// file is written by a program that adds keys freely, and one unexpected shape
/// in one connection must not cost the user the other eleven. Every field is
/// optional here, and a connection with nothing usable in it is [`Skipped`] by
/// name rather than dropped silently.
///
/// Folders are ignored — DBeaver groups connections, this app doesn't.
/// Credentials live encrypted in the sibling `credentials-config.json`, which
/// this deliberately does not touch.
pub fn parse_dbeaver(json: &str) -> ImportScan {
    let mut out = ImportScan::default();
    let root: serde_json::Value = match serde_json::from_str(strip_bom(json)) {
        Ok(v) => v,
        Err(e) => {
            out.skip("", SkipReason::Unreadable(e.to_string()));
            return out;
        }
    };
    let Some(conns) = root.get("connections").and_then(|v| v.as_object()) else {
        return out;
    };

    for (id, entry) in conns {
        let name = str_at(entry, "name").unwrap_or_else(|| id.clone());
        let provider = str_at(entry, "provider").unwrap_or_default();
        let driver = str_at(entry, "driver").unwrap_or_default();
        let cfg = entry.get("configuration");
        let url = cfg.and_then(|c| str_at(c, "url")).unwrap_or_default();

        let Some(engine) = dbeaver_engine(&provider, &driver, &url) else {
            let named = if driver.is_empty() { provider } else { driver };
            out.skip(name, SkipReason::UnsupportedEngine(named));
            continue;
        };

        // Start from the JDBC URL when it parses — it is the one field that
        // carries the query parameters — then let the explicit fields win, since
        // DBeaver edits those and rewrites the URL from them.
        let mut c = match parse_url(&url) {
            Ok(c) if same_engine(&c.db_type, engine) => c,
            _ => blank(engine),
        };
        c.db_type = engine.to_string();
        if let Some(cfg) = cfg {
            overlay(&mut c.host, str_at(cfg, "host"));
            if let Some(p) = str_at(cfg, "port").and_then(|p| p.trim().parse::<u16>().ok()) {
                c.port = p;
            }
            overlay(&mut c.user, str_at(cfg, "user"));
            let database = str_at(cfg, "database");
            if is_sqlite(engine) {
                // SQLite's "database" is the file; DBeaver puts the path there.
                overlay(&mut c.file, database);
            } else {
                overlay(&mut c.database, database);
            }
            dbeaver_handlers(&mut c, cfg);
        }
        if !is_sqlite(engine) && c.host.trim().is_empty() {
            out.skip(name, SkipReason::NoServer);
            continue;
        }
        if is_sqlite(engine) && c.file.trim().is_empty() {
            out.skip(name, SkipReason::NoServer);
            continue;
        }
        c.name = if name.trim().is_empty() {
            suggest_name(&c)
        } else {
            name
        };
        out.found.push(imported(c, ImportSource::DBeaver));
    }
    // DBeaver keys its connections by an internal id, so "the order in the file"
    // is not an order a user ever chose or would recognise — and `serde_json`
    // hands the object back sorted by that id, which is worse than arbitrary
    // because it looks deliberate. Sort by the one thing the reader is scanning.
    out.found.sort_by_key(|i| i.connection.name.to_lowercase());
    out.skipped.sort_by_key(|s| s.name.to_lowercase());
    out
}

/// DBeaver names the engine twice — a provider and a driver — and only the
/// driver distinguishes MariaDB, which ships under the `mysql` provider.
fn dbeaver_engine(provider: &str, driver: &str, url: &str) -> Option<&'static str> {
    let p = provider.to_ascii_lowercase();
    let d = driver.to_ascii_lowercase();
    if d.contains("maria") || p.contains("maria") {
        return Some(MARIADB);
    }
    match p.as_str() {
        "mysql" => Some(MYSQL),
        "postgresql" | "postgres" => Some(POSTGRES),
        "sqlite" => Some(SQLITE),
        // A generic/JDBC data source names its engine only in the URL.
        _ => split_scheme(strip_prefix_ci(url, "jdbc:").unwrap_or(url))
            .and_then(|(s, _)| engine_for_scheme(s)),
    }
}

/// DBeaver's per-connection "handlers" — the SSH tunnel and the SSL block.
///
/// Best-effort by design: the property names differ per driver, so this reads
/// the ones that are stable and leaves the rest for the user to check in a form
/// that now at least has the host in it.
fn dbeaver_handlers(c: &mut Connection, cfg: &serde_json::Value) {
    let Some(handlers) = cfg.get("handlers").and_then(|h| h.as_object()) else {
        return;
    };
    for (key, handler) in handlers {
        let enabled = handler
            .get("enabled")
            .and_then(|e| e.as_bool())
            .unwrap_or(false);
        if !enabled {
            continue;
        }
        let props = handler.get("properties");
        let prop = |name: &str| props.and_then(|p| str_at(p, name)).unwrap_or_default();
        let k = key.to_ascii_lowercase();
        if k.contains("ssh") {
            c.ssh = SshTunnel {
                enabled: true,
                host: prop("host"),
                port: prop("port").trim().parse::<u16>().unwrap_or(22),
                user: prop("userName"),
                password: String::new(),
                auth: if prop("authType").to_ascii_uppercase().contains("PUBLIC") {
                    SshAuth::KeyPair
                } else {
                    SshAuth::Password
                },
                key_path: prop("keyPath"),
                key_passphrase: String::new(),
            };
        } else if k.contains("ssl") {
            // An enabled SSL handler with no mode of its own means "encrypt" and
            // nothing about verification, which is exactly `Require`.
            c.tls.mode = sslmode_from_str(&prop("ssl.mode")).unwrap_or(SslMode::Require);
            set_if_empty(&mut c.tls.ca_path, &prop("ssl.ca.cert"));
            set_if_empty(&mut c.tls.ca_path, &prop("ssl.root.cert"));
            set_if_empty(&mut c.tls.client_cert_path, &prop("ssl.client.cert"));
            set_if_empty(&mut c.tls.client_key_path, &prop("ssl.client.key"));
        }
    }
}

fn str_at(v: &serde_json::Value, key: &str) -> Option<String> {
    match v.get(key) {
        Some(serde_json::Value::String(s)) => Some(s.clone()),
        Some(serde_json::Value::Number(n)) => Some(n.to_string()),
        _ => None,
    }
}

fn overlay(dst: &mut String, v: Option<String>) {
    if let Some(v) = v
        && !v.trim().is_empty()
    {
        *dst = v;
    }
}

// ---------------------------------------------------------------------------
// DataGrip / JetBrains
// ---------------------------------------------------------------------------

/// Parse a JetBrains `dataSources.xml` (DataGrip's, or the one an IDE writes
/// into a project's `.idea/`).
///
/// A narrow element scan rather than a real XML parse, and that is a decision
/// with a cost: it reads exactly `<data-source>`'s `name` attribute and its
/// `<jdbc-url>`, `<user-name>` and `<driver-ref>` children, and would be
/// defeated by either of those inside a comment or a CDATA section. The
/// alternative is a new dependency in `schemaic-core` — which has none for
/// parsing at all — for one file whose shape JetBrains has written the same way
/// for a decade. If that shape ever moves, this reports zero connections rather
/// than wrong ones, which is the failure mode to prefer.
///
/// Passwords are in the OS credential store, not here.
pub fn parse_datagrip(xml: &str) -> ImportScan {
    let mut out = ImportScan::default();
    for block in elements(strip_bom(xml), "data-source") {
        let name = attribute(block.head, "name").unwrap_or_default();
        let url = child_text(block.body, "jdbc-url").unwrap_or_default();
        if url.trim().is_empty() {
            out.skip(
                pick_name(&name, "(unnamed data source)"),
                SkipReason::NoServer,
            );
            continue;
        }
        let mut c = match parse_url(&url) {
            Ok(c) => c,
            Err(UrlError::UnknownScheme(s)) => {
                out.skip(
                    pick_name(&name, &url),
                    SkipReason::UnsupportedEngine(driver_name(block.body, &s)),
                );
                continue;
            }
            Err(e) => {
                out.skip(pick_name(&name, &url), SkipReason::Unreadable(e.message()));
                continue;
            }
        };
        if let Some(u) = child_text(block.body, "user-name") {
            set_if_empty(&mut c.user, u.trim());
        }
        if !name.trim().is_empty() {
            c.name = name;
        }
        let mut imp = imported(c, ImportSource::DataGrip);
        if has_macro(&url) {
            imp.note(ImportNote::UnexpandedPath);
        }
        out.found.push(imp);
    }
    out
}

/// The `<driver-ref>` if there is one, so an unsupported entry is reported as
/// "oracle.16" rather than as the bare scheme.
fn driver_name(body: &str, fallback: &str) -> String {
    child_text(body, "driver-ref")
        .filter(|d| !d.trim().is_empty())
        .unwrap_or_else(|| fallback.to_string())
}

fn pick_name(name: &str, fallback: &str) -> String {
    if name.trim().is_empty() {
        fallback.to_string()
    } else {
        name.to_string()
    }
}

/// A JetBrains path macro the IDE expands against a project it knows and we
/// don't.
fn has_macro(s: &str) -> bool {
    s.contains("$PROJECT_DIR$") || s.contains("$USER_HOME$") || s.contains("$APPLICATION_")
}

/// One `<tag …>body</tag>` occurrence.
struct Element<'a> {
    /// Everything between `<tag` and the closing `>` — the attributes.
    head: &'a str,
    /// Everything between `>` and `</tag>`; empty for a self-closing element.
    body: &'a str,
}

/// Every non-nested `<tag>` element in `xml`, in document order.
fn elements<'a>(xml: &'a str, tag: &str) -> Vec<Element<'a>> {
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(i) = rest.find(&open) {
        let after = &rest[i + open.len()..];
        // `<data-sources>` must not match `<data-source`.
        if !after.starts_with([' ', '\t', '\r', '\n', '>', '/']) {
            rest = after;
            continue;
        }
        let Some(gt) = after.find('>') else { break };
        let head = &after[..gt];
        let tail = &after[gt + 1..];
        if head.trim_end().ends_with('/') {
            out.push(Element { head, body: "" });
            rest = tail;
            continue;
        }
        match tail.find(&close) {
            Some(end) => {
                out.push(Element {
                    head,
                    body: &tail[..end],
                });
                rest = &tail[end + close.len()..];
            }
            None => {
                out.push(Element { head, body: tail });
                break;
            }
        }
    }
    out
}

/// `name="value"` (or `name='value'`) out of an element's attribute text.
fn attribute(head: &str, name: &str) -> Option<String> {
    let mut rest = head;
    while let Some(i) = rest.find(name) {
        let before_ok = i == 0 || rest.as_bytes()[i - 1].is_ascii_whitespace();
        let after = &rest[i + name.len()..];
        let after_trimmed = after.trim_start();
        if before_ok && let Some(v) = after_trimmed.strip_prefix('=') {
            let v = v.trim_start();
            for q in ['"', '\''] {
                if let Some(inner) = v.strip_prefix(q)
                    && let Some(end) = inner.find(q)
                {
                    return Some(xml_unescape(&inner[..end]));
                }
            }
            // An unquoted value: not something this scanner can delimit, so
            // keep looking rather than answering `None` on its behalf. Giving up
            // here would be a second rule inside the loop that exists to find a
            // *well-formed* attribute.
        }
        rest = after;
    }
    None
}

/// The text of the first `<tag>…</tag>` inside `body`.
fn child_text(body: &str, tag: &str) -> Option<String> {
    elements(body, tag).first().map(|e| xml_unescape(e.body))
}

fn xml_unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let after = &rest[i..];
        let Some(semi) = after.find(';').filter(|n| *n <= 12) else {
            out.push('&');
            rest = &after[1..];
            continue;
        };
        let entity = &after[1..semi];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => entity
                .strip_prefix('#')
                .and_then(|n| match n.strip_prefix(['x', 'X']) {
                    Some(hex) => u32::from_str_radix(hex, 16).ok(),
                    None => n.parse::<u32>().ok(),
                })
                .and_then(char::from_u32),
        };
        match decoded {
            Some(ch) => {
                out.push(ch);
                rest = &after[semi + 1..];
            }
            None => {
                out.push('&');
                rest = &after[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

// ---------------------------------------------------------------------------
// INI-shaped files: .my.cnf and .pg_service.conf
// ---------------------------------------------------------------------------

/// `[section]` groups and their `key = value` pairs, in file order.
///
/// One scanner for both INI-shaped sources — `.my.cnf` and `.pg_service.conf`
/// differ in what their keys *mean*, not in how they are written, and two
/// scanners would be two sets of edge cases with one of them untested. Pairs
/// before the first section are returned under an empty name.
fn ini_sections(text: &str) -> Vec<(String, Vec<(String, String)>)> {
    let mut out: Vec<(String, Vec<(String, String)>)> = vec![(String::new(), Vec::new())];
    for line in strip_bom(text).lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with(['#', ';']) {
            continue;
        }
        // `!include` / `!includedir` pull in another file; this parser is given
        // one file's text and has no way to follow them.
        if line.starts_with('!') {
            continue;
        }
        if let Some(inner) = line.strip_prefix('[').and_then(|l| l.split(']').next()) {
            out.push((inner.trim().to_string(), Vec::new()));
            continue;
        }
        let (key, value) = match line.split_once('=') {
            // A bare flag (`quick`, `no-auto-rehash`); keep it with an empty
            // value so a caller that cares can see it was set.
            None => (line, ""),
            Some((k, v)) => (k, v),
        };
        let value = unquote(value.trim());
        out.last_mut()
            .expect("seeded above")
            .1
            .push((key.trim().to_string(), value));
    }
    out
}

fn unquote(v: &str) -> String {
    for q in ['"', '\''] {
        if v.len() >= 2
            && let Some(inner) = v.strip_prefix(q).and_then(|t| t.strip_suffix(q))
        {
            return inner.to_string();
        }
    }
    v.to_string()
}

/// Look a key up by [`normalize_key`], last write winning as the file's readers
/// do.
fn ini_get(pairs: &[(String, String)], key: &str) -> Option<String> {
    let want = normalize_key(key);
    pairs
        .iter()
        .rev()
        .find(|(k, _)| normalize_key(k) == want)
        .map(|(_, v)| v.clone())
}

/// Parse a `~/.my.cnf`.
///
/// `[client]` is the base every client group inherits, so a group named
/// `client_prod` is read *layered on top of it* — that is what
/// `--defaults-group-suffix=_prod` does, and reading the group alone would drop
/// the shared host. `[mysqld]` is the server's own configuration and is not a
/// client at all.
///
/// A file with credentials but no `host` means localhost, which is what the
/// command-line client does with it; that is the single most common shape this
/// file has.
pub fn parse_my_cnf(text: &str) -> ImportScan {
    let mut out = ImportScan::default();
    let sections = ini_sections(text);
    let base: Vec<(String, String)> = sections
        .iter()
        .filter(|(name, _)| name == "client")
        .flat_map(|(_, pairs)| pairs.clone())
        .collect();

    for (name, pairs) in &sections {
        if !is_client_group(name) {
            continue;
        }
        let mut merged = base.clone();
        if name != "client" {
            merged.extend(pairs.clone());
        }
        let Some(mut c) = my_cnf_connection(&merged) else {
            continue;
        };
        c.name = if name == "client" {
            suggest_name(&c)
        } else {
            name.clone()
        };
        out.found.push(imported(c, ImportSource::MyCnf));
    }
    out.found = dedupe(std::mem::take(&mut out.found));
    out
}

/// Which `.my.cnf` groups describe a client connection.
///
/// `client` is read by every client; `mysql` by the command-line one; a
/// `client`-prefixed group is a named alternative selected with
/// `--defaults-group-suffix`.
fn is_client_group(name: &str) -> bool {
    name.starts_with("client") || name == "mysql"
}

fn my_cnf_connection(pairs: &[(String, String)]) -> Option<Connection> {
    let host = ini_get(pairs, "host").unwrap_or_default();
    let user = ini_get(pairs, "user").unwrap_or_default();
    let password = ini_get(pairs, "password").unwrap_or_default();
    let database = ini_get(pairs, "database").unwrap_or_default();
    // A group with none of the four says nothing about a connection.
    if host.is_empty() && user.is_empty() && password.is_empty() && database.is_empty() {
        return None;
    }
    let mut c = blank(MYSQL);
    c.host = if host.trim().is_empty() {
        "127.0.0.1".to_string()
    } else {
        host
    };
    if let Some(p) = ini_get(pairs, "port").and_then(|p| p.trim().parse::<u16>().ok()) {
        c.port = p;
    }
    c.user = user;
    c.password = password;
    c.database = database;
    if let Some(m) = ini_get(pairs, "ssl-mode").and_then(|m| sslmode_from_str(&m)) {
        c.tls.mode = m;
    }
    if let Some(ca) = ini_get(pairs, "ssl-ca") {
        c.tls.ca_path = ca;
    }
    if let Some(cert) = ini_get(pairs, "ssl-cert") {
        c.tls.client_cert_path = cert;
    }
    if let Some(key) = ini_get(pairs, "ssl-key") {
        c.tls.client_key_path = key;
    }
    Some(c)
}

/// Parse a `~/.pg_service.conf` — libpq's named services, one connection each.
///
/// The section name *is* the connection's name here, which makes this the one
/// plain-text source that arrives already named the way its author named it.
pub fn parse_pg_service(text: &str) -> ImportScan {
    let mut out = ImportScan::default();
    for (name, pairs) in ini_sections(text) {
        if name.is_empty() {
            continue;
        }
        let host = ini_get(&pairs, "host")
            .or_else(|| ini_get(&pairs, "hostaddr"))
            .unwrap_or_default();
        if host.trim().is_empty() {
            out.skip(name, SkipReason::NoServer);
            continue;
        }
        let mut c = blank(POSTGRES);
        c.host = host;
        if let Some(p) = ini_get(&pairs, "port").and_then(|p| p.trim().parse::<u16>().ok()) {
            c.port = p;
        }
        c.user = ini_get(&pairs, "user").unwrap_or_default();
        c.password = ini_get(&pairs, "password").unwrap_or_default();
        c.database = ini_get(&pairs, "dbname").unwrap_or_default();
        if let Some(m) = ini_get(&pairs, "sslmode").and_then(|m| sslmode_from_str(&m)) {
            c.tls.mode = m;
        }
        if let Some(ca) = ini_get(&pairs, "sslrootcert") {
            c.tls.ca_path = ca;
        }
        if let Some(cert) = ini_get(&pairs, "sslcert") {
            c.tls.client_cert_path = cert;
        }
        if let Some(key) = ini_get(&pairs, "sslkey") {
            c.tls.client_key_path = key;
        }
        c.name = name;
        out.found.push(imported(c, ImportSource::PgService));
    }
    out
}

// ---------------------------------------------------------------------------
// .pgpass
// ---------------------------------------------------------------------------

/// One line of a `~/.pgpass`: `hostname:port:database:username:password`.
///
/// Fields are kept as written, wildcards included — `*` is a *pattern*, and
/// resolving it against a connection is [`Self::matches`]'s job, not the
/// parser's.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PgpassEntry {
    /// Host name, or `*`.
    pub host: String,
    /// Port, or `*`.
    pub port: String,
    /// Database, or `*`.
    pub database: String,
    /// User, or `*`.
    pub user: String,
    /// The password. Never a wildcard.
    pub password: String,
}

impl PgpassEntry {
    /// Does this line supply `c`'s password?
    ///
    /// libpq's own rule: each field matches the connection's or is `*`. The one
    /// addition is that a connection with **no user named yet** matches any
    /// user line — an imported DataGrip row often has the server and not the
    /// login, and that is precisely the row this file can complete. See
    /// [`fill_missing_passwords`], which adopts the user along with the
    /// password so the pair stays consistent.
    pub fn matches(&self, c: &Connection) -> bool {
        is_postgres(&c.db_type)
            && wild(&self.host, &c.host)
            && (self.port == "*" || self.port == c.port.to_string())
            && wild(&self.database, &c.database)
            && (self.user == "*" || c.user.trim().is_empty() || self.user == c.user)
    }
}

fn wild(pattern: &str, value: &str) -> bool {
    pattern == "*" || pattern.eq_ignore_ascii_case(value)
}

/// Parse a `~/.pgpass` into its lines.
///
/// A backslash escapes a `:` or another backslash inside a field, which is the
/// only reason this can't be `split(':')`.
pub fn pgpass_entries(text: &str) -> Vec<PgpassEntry> {
    let mut out = Vec::new();
    for line in strip_bom(text).lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let fields = split_pgpass_line(trimmed);
        if fields.len() < 5 {
            continue;
        }
        out.push(PgpassEntry {
            host: fields[0].clone(),
            port: fields[1].clone(),
            database: fields[2].clone(),
            user: fields[3].clone(),
            // A password may itself contain `:`, so everything after the fourth
            // separator is the password — `split_pgpass_line` stops splitting
            // once it has four.
            password: fields[4].clone(),
        });
    }
    out
}

fn split_pgpass_line(line: &str) -> Vec<String> {
    let mut fields = vec![String::new()];
    let mut chars = line.chars();
    while let Some(ch) = chars.next() {
        match ch {
            '\\' => {
                if let Some(next) = chars.next() {
                    fields.last_mut().expect("seeded above").push(next);
                }
            }
            ':' if fields.len() < 5 => fields.push(String::new()),
            _ => fields.last_mut().expect("seeded above").push(ch),
        }
    }
    fields
}

/// Parse a `~/.pgpass` as a source of connections in its own right.
///
/// A line with a wildcard host names every server and therefore none, so it is
/// [`SkipReason::NoServer`] — but it is still returned by [`pgpass_entries`] and
/// still completes other sources' rows through [`fill_missing_passwords`], which
/// is where a `*:*:*:postgres:secret` line earns its keep.
pub fn parse_pgpass(text: &str) -> ImportScan {
    let mut out = ImportScan::default();
    for e in pgpass_entries(text) {
        if e.host == "*" {
            out.skip(format!("{}@*", e.user), SkipReason::NoServer);
            continue;
        }
        let mut c = blank(POSTGRES);
        c.host = e.host;
        if let Ok(p) = e.port.parse::<u16>() {
            c.port = p;
        }
        c.database = if e.database == "*" {
            String::new()
        } else {
            e.database
        };
        c.user = if e.user == "*" { String::new() } else { e.user };
        c.password = e.password;
        c.name = suggest_name(&c);
        out.found.push(imported(c, ImportSource::Pgpass));
    }
    out
}

/// Complete every password-less PostgreSQL row from the `.pgpass` lines.
///
/// This is what makes a DataGrip or DBeaver import usable rather than a list of
/// forms to finish by hand: neither tool writes its passwords where we can read
/// them, and libpq's file — on the same machine, for the same servers — has
/// them in plaintext by design.
///
/// Adopts the line's **user** when the row named none, because a password
/// without the login it belongs to is not a credential.
///
/// Filling a row also **retracts its [`ImportNote::NoPassword`]**. [`scan`] runs
/// this before it adds those notes, so there the order alone would do — but a
/// caller completing a row it has already scanned (a hand-picked file, which
/// gets its passwords afterwards) would otherwise show "No password in the
/// source" on a row that has one. The function that supplies the password is the
/// one that knows the claim is no longer true.
pub fn fill_missing_passwords(found: &mut [Imported], entries: &[PgpassEntry]) {
    for imp in found.iter_mut() {
        if !imp.connection.password.is_empty() {
            continue;
        }
        let Some(e) = entries.iter().find(|e| e.matches(&imp.connection)) else {
            continue;
        };
        imp.connection.password = e.password.clone();
        if imp.connection.user.trim().is_empty() && e.user != "*" {
            imp.connection.user = e.user.clone();
        }
        imp.notes.retain(|n| *n != ImportNote::NoPassword);
    }
}

// ---------------------------------------------------------------------------
// Identity: duplicates within a scan, and against what is already saved
// ---------------------------------------------------------------------------

/// Do these two connections point at the same place, as the same user?
///
/// The identity an import needs, and deliberately not the identity the app uses
/// elsewhere — `conn_id` answers "is this the same saved connection", which a
/// row that has never been saved cannot have. Name and colour are excluded: two
/// tools naming one server differently is the normal case, and it is exactly the
/// case where a second copy is not wanted.
///
/// The user is part of it. Two logins on one server are two connections — that
/// is how a read-only reporting account is kept beside the owner's.
pub fn same_endpoint(a: &Connection, b: &Connection) -> bool {
    if !same_engine(&a.db_type, &b.db_type) {
        return false;
    }
    if is_sqlite(&a.db_type) {
        return same_path(&a.file, &b.file);
    }
    a.host.eq_ignore_ascii_case(&b.host)
        && a.port == b.port
        && a.database.eq_ignore_ascii_case(&b.database)
        && a.user == b.user
}

/// Path comparison for the one engine whose target is a file. Separators are
/// normalised because a JDBC URL writes `/` on a platform whose own paths use
/// `\`, and the two name one file.
fn same_path(a: &str, b: &str) -> bool {
    let norm = |s: &str| s.replace('\\', "/");
    norm(a).eq_ignore_ascii_case(&norm(b))
}

/// Drop rows that repeat an earlier row's endpoint, keeping the **first**.
///
/// Sources are parsed in a fixed order and the first one to describe a server is
/// the one that described it best — a `.pgpass` line is the same endpoint as the
/// DBeaver connection above it, with a worse name. Runs after
/// [`fill_missing_passwords`] so the survivor has already collected the password
/// the row below it was carrying.
pub fn dedupe(found: Vec<Imported>) -> Vec<Imported> {
    let mut out: Vec<Imported> = Vec::with_capacity(found.len());
    for imp in found {
        if out
            .iter()
            .any(|k| same_endpoint(&k.connection, &imp.connection))
        {
            continue;
        }
        out.push(imp);
    }
    out
}

/// Fold newly-parsed rows into a review list already on screen, and answer which
/// of them should be **ticked**.
///
/// The rule every source shares, and the reason it lives here rather than at the
/// three call sites: a paste, a picked file and a client scan all add to the same
/// list, and the moment they disagree about ticking or about duplicates the user
/// gets a different answer depending on which button they pressed.
///
/// **Appends, never replaces.** The UI keys its selection by *index into this
/// list*, so the one guarantee that makes those indices safe is that the list
/// only ever grows at the end. It is also why a scan cannot discard a URL pasted
/// before it finished.
///
/// A row repeating an endpoint already present selects **that** row instead of
/// adding a second — pasting a URL for a server the scan already found would
/// otherwise look like nothing happened at all.
///
/// Ticking asks [`Imported::preselected`] of whichever row ends up in the list,
/// so a row duplicating a *saved* connection arrives unticked whatever produced
/// it. Re-adding a server the user already has is the one outcome nobody wants
/// by accident, and it must not depend on how the row got here.
pub fn merge_rows(into: &mut Vec<Imported>, found: Vec<Imported>) -> Vec<usize> {
    let mut tick = Vec::new();
    for row in found {
        let at = match into
            .iter()
            .position(|k| same_endpoint(&k.connection, &row.connection))
        {
            Some(i) => i,
            None => {
                into.push(row);
                into.len() - 1
            }
        };
        if into[at].preselected() {
            tick.push(at);
        }
    }
    tick
}

/// Fold more skipped entries into the ones already being reported, **without
/// repeating any**.
///
/// The count in "N entries were not imported" is a claim about the sources, not
/// about how many times they were read: scanning twice, or picking the same file
/// twice, must not turn three Oracle data sources into six. [`merge_rows`]
/// collapses repeats by endpoint and this is its other half — without it the two
/// halves of one scan's result disagree about what a second scan means.
pub fn merge_skipped(into: &mut Vec<Skipped>, more: Vec<Skipped>) {
    for entry in more {
        if !into.contains(&entry) {
            into.push(entry);
        }
    }
}

/// Flag every row the user already has, so the list can leave it unticked
/// instead of quietly adding a thirteenth copy of the same server.
pub fn mark_existing(found: &mut [Imported], existing: &[Connection]) {
    for imp in found.iter_mut() {
        if existing.iter().any(|e| same_endpoint(e, &imp.connection)) {
            imp.note(ImportNote::AlreadySaved);
        }
    }
}

// ---------------------------------------------------------------------------
// Shared construction
// ---------------------------------------------------------------------------

/// A connection with nothing filled in but its engine and that engine's port.
///
/// Written out field by field rather than through a `Default`: adding a field to
/// `Connection` should make *this* fail to compile, so an import decides what
/// the new field is rather than inheriting whatever `Default` would say.
fn blank(db_type: &str) -> Connection {
    Connection {
        id: 0,
        name: String::new(),
        db_type: db_type.to_string(),
        host: String::new(),
        port: default_port(db_type),
        user: String::new(),
        password: String::new(),
        file: String::new(),
        database: String::new(),
        ssh: SshTunnel::default(),
        tls: Tls::default(),
        color: None,
        prominent_color: false,
        read_only: false,
        environment: Environment::default(),
        ai_data: None,
    }
}

fn imported(connection: Connection, source: ImportSource) -> Imported {
    Imported {
        connection,
        source,
        origin: String::new(),
        notes: Vec::new(),
    }
}

/// A name for a connection whose source gave it none.
///
/// `database@host` where there is a database, the host alone otherwise, and the
/// file's own name on SQLite — the same subtitle `Connection::endpoint` shows,
/// because a name that repeats what is under it is still better than "New
/// connection 4".
fn suggest_name(c: &Connection) -> String {
    if is_sqlite(&c.db_type) {
        let base = c
            .file
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or("")
            .trim()
            .to_string();
        return if base.is_empty() {
            "SQLite database".to_string()
        } else {
            base
        };
    }
    let host = c.host.trim();
    let db = c.database.trim();
    match (db.is_empty(), host.is_empty()) {
        (false, false) => format!("{db}@{host}"),
        (true, false) => host.to_string(),
        (false, true) => db.to_string(),
        (true, true) => "Imported connection".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(s: &str) -> Connection {
        parse_url(s).expect("should parse")
    }

    // -- URL ----------------------------------------------------------------

    #[test]
    fn a_mysql_url_fills_every_component() {
        let c = url("mysql://root:s3cret@db.internal:3307/shop");
        assert_eq!(c.db_type, "MySQL");
        assert_eq!(c.host, "db.internal");
        assert_eq!(c.port, 3307);
        assert_eq!(c.user, "root");
        assert_eq!(c.password, "s3cret");
        assert_eq!(c.database, "shop");
        assert_eq!(c.name, "shop@db.internal");
    }

    #[test]
    fn a_missing_port_takes_the_engines_default() {
        assert_eq!(url("mysql://h/d").port, 3306);
        assert_eq!(url("postgresql://h/d").port, 5432);
    }

    #[test]
    fn every_postgres_scheme_alias_lands_on_one_label() {
        for s in ["postgres", "postgresql", "pgsql", "psql"] {
            let c = url(&format!("{s}://h/d"));
            assert!(is_postgres(&c.db_type), "{s}");
        }
        // SQLAlchemy's driver selector names the same server.
        assert!(is_postgres(&url("postgresql+psycopg2://h/d").db_type));
    }

    #[test]
    fn mariadb_keeps_its_own_label() {
        assert_eq!(url("mariadb://h/d").db_type, "MariaDB");
    }

    #[test]
    fn a_jdbc_wrapper_is_stripped_and_its_params_are_read() {
        let c = url("jdbc:mysql://localhost:3306/sakila?user=u&password=p&useSSL=true");
        assert_eq!(c.host, "localhost");
        assert_eq!(c.database, "sakila");
        assert_eq!(c.user, "u");
        assert_eq!(c.password, "p");
        assert_eq!(c.tls.mode, SslMode::Require);
    }

    #[test]
    fn the_authority_beats_a_query_parameter_naming_the_same_thing() {
        let c = url("mysql://real:pw@h/d?user=other&password=other");
        assert_eq!(c.user, "real");
        assert_eq!(c.password, "pw");
    }

    #[test]
    fn percent_escapes_in_the_credentials_are_decoded() {
        let c = url("postgres://us%65r:p%40ss%2Fword@h:5432/d%2Db");
        assert_eq!(c.user, "user");
        assert_eq!(c.password, "p@ss/word");
        assert_eq!(c.database, "d-b");
    }

    #[test]
    fn a_password_holding_an_at_sign_splits_at_the_last_one() {
        let c = url("mysql://u:p@ss@h/d");
        assert_eq!(c.user, "u");
        assert_eq!(c.password, "p@ss");
        assert_eq!(c.host, "h");
    }

    #[test]
    fn a_bracketed_ipv6_host_keeps_its_colons() {
        let c = url("postgres://u@[2001:db8::1]:5433/d");
        assert_eq!(c.host, "2001:db8::1");
        assert_eq!(c.port, 5433);
        let c = url("postgres://u@[::1]/d");
        assert_eq!(c.host, "::1");
        assert_eq!(c.port, 5432);
    }

    #[test]
    fn a_multi_host_libpq_url_takes_the_first_server() {
        let c = url("postgresql://u@a.example:5432,b.example:5432/d");
        assert_eq!(c.host, "a.example");
        assert_eq!(c.port, 5432);
    }

    #[test]
    fn an_env_assignment_around_the_url_is_stripped() {
        for line in [
            "DATABASE_URL=postgres://u:p@h:5432/d",
            "export DATABASE_URL=postgres://u:p@h:5432/d",
            "DATABASE_URL=\"postgres://u:p@h:5432/d\"",
            "  DATABASE_URL='postgres://u:p@h:5432/d'  ",
        ] {
            let c = url(line);
            assert_eq!(c.host, "h", "{line}");
            assert_eq!(c.password, "p", "{line}");
        }
    }

    #[test]
    fn a_query_password_survives_the_env_prefix_stripping() {
        // The `=` inside the URL must not be mistaken for the assignment's.
        let c = url("jdbc:mysql://h/d?password=pw");
        assert_eq!(c.password, "pw");
    }

    #[test]
    fn sqlite_urls_resolve_to_a_file_path() {
        assert_eq!(url("sqlite:///var/db/app.db").file, "/var/db/app.db");
        assert_eq!(url("sqlite://relative.db").file, "relative.db");
        assert_eq!(url("sqlite:relative.db").file, "relative.db");
        assert_eq!(url("jdbc:sqlite:/home/me/app.db").file, "/home/me/app.db");
        assert_eq!(url("sqlite:///C:/data/app.db").file, "C:/data/app.db");
        assert_eq!(
            url("sqlite:///var/db/app.db?mode=ro").file,
            "/var/db/app.db"
        );
    }

    #[test]
    fn a_sqlite_connection_is_named_after_its_file() {
        assert_eq!(url("sqlite:///var/db/app.db").name, "app.db");
    }

    #[test]
    fn sslmode_maps_both_vocabularies_onto_the_ladder() {
        assert_eq!(
            url("postgres://h/d?sslmode=verify-full").tls.mode,
            SslMode::VerifyFull
        );
        assert_eq!(
            url("postgres://h/d?sslmode=require").tls.mode,
            SslMode::Require
        );
        assert_eq!(
            url("mysql://h/d?ssl-mode=VERIFY_IDENTITY").tls.mode,
            SslMode::VerifyFull
        );
        assert_eq!(
            url("mysql://h/d?sslMode=DISABLED").tls.mode,
            SslMode::Disable
        );
        // Unrecognised leaves the default alone rather than guessing.
        assert_eq!(
            url("postgres://h/d?sslmode=elsewhere").tls.mode,
            SslMode::Disable
        );
    }

    #[test]
    fn certificate_paths_come_across_from_the_query() {
        let c = url(
            "postgres://h/d?sslmode=verify-ca&sslrootcert=/ca.pem&sslcert=/c.pem&sslkey=/k.pem",
        );
        assert_eq!(c.tls.ca_path, "/ca.pem");
        assert_eq!(c.tls.client_cert_path, "/c.pem");
        assert_eq!(c.tls.client_key_path, "/k.pem");
    }

    #[test]
    fn url_failures_are_named_rather_than_swallowed() {
        assert_eq!(parse_url("   "), Err(UrlError::Empty));
        assert_eq!(parse_url("localhost:3306"), Err(UrlError::NoScheme));
        assert_eq!(
            parse_url("oracle://h:1521/x"),
            Err(UrlError::UnknownScheme("oracle".to_string()))
        );
        assert_eq!(parse_url("mysql:///d"), Err(UrlError::NoHost));
        assert_eq!(
            parse_url("mysql://h:notaport/d"),
            Err(UrlError::BadPort("notaport".to_string()))
        );
        assert_eq!(parse_url("sqlite://"), Err(UrlError::NoPath));
    }

    #[test]
    fn a_bare_host_port_is_not_read_as_a_scheme() {
        // `localhost:3306` splits at a colon like a URL does; the scheme test is
        // what stops `localhost` being treated as an engine name.
        assert!(matches!(
            parse_url("localhost:3306"),
            Err(UrlError::NoScheme)
        ));
        assert!(matches!(
            parse_url("127.0.0.1:5432"),
            Err(UrlError::NoScheme)
        ));
    }

    // -- DBeaver ------------------------------------------------------------

    const DBEAVER: &str = r#"{
      "folders": { "Work": {} },
      "connections": {
        "mysql-1": {
          "provider": "mysql", "driver": "mysql8", "name": "Shop (prod)",
          "configuration": {
            "host": "shop.example", "port": "3306", "database": "shop",
            "user": "app",
            "url": "jdbc:mysql://shop.example:3306/shop",
            "handlers": {
              "ssh_tunnel": {
                "enabled": true,
                "properties": { "host": "bastion.example", "port": "2222",
                                "userName": "jump", "authType": "PUBLIC_KEY",
                                "keyPath": "/home/me/.ssh/id_ed25519" }
              }
            }
          }
        },
        "pg-2": {
          "provider": "postgresql", "driver": "postgres-jdbc", "name": "Analytics",
          "configuration": {
            "host": "pg.example", "port": "5433", "database": "analytics",
            "url": "jdbc:postgresql://pg.example:5433/analytics",
            "handlers": { "ssl": { "enabled": true,
                                   "properties": { "ssl.mode": "verify-ca",
                                                   "ssl.ca.cert": "/ca.pem" } } }
          }
        },
        "maria-3": {
          "provider": "mysql", "driver": "mariaDB", "name": "Legacy",
          "configuration": { "host": "old.example", "port": "3306", "database": "legacy" }
        },
        "sqlite-4": {
          "provider": "sqlite", "driver": "sqlite_jdbc", "name": "Notes",
          "configuration": { "database": "/home/me/notes.db",
                             "url": "jdbc:sqlite:/home/me/notes.db" }
        },
        "oracle-5": {
          "provider": "oracle", "driver": "oracle_thin", "name": "Warehouse",
          "configuration": { "host": "ora.example", "port": "1521" }
        }
      }
    }"#;

    /// The row DBeaver's fixture calls `name`. Looked up rather than indexed:
    /// the file is a JSON *object*, so its entries have no order of their own —
    /// which is why `parse_dbeaver` imposes one.
    fn row<'a>(scan: &'a ImportScan, name: &str) -> &'a Connection {
        &scan
            .found
            .iter()
            .find(|i| i.connection.name == name)
            .unwrap_or_else(|| panic!("no row named {name}"))
            .connection
    }

    #[test]
    fn dbeaver_reads_the_four_engines_it_can_and_skips_the_one_it_cannot() {
        let scan = parse_dbeaver(DBEAVER);
        let names: Vec<&str> = scan
            .found
            .iter()
            .map(|i| i.connection.name.as_str())
            .collect();
        // Sorted by name, not by DBeaver's internal ids.
        assert_eq!(names, ["Analytics", "Legacy", "Notes", "Shop (prod)"]);
        assert_eq!(scan.skipped.len(), 1);
        assert_eq!(scan.skipped[0].name, "Warehouse");
        assert_eq!(
            scan.skipped[0].reason,
            SkipReason::UnsupportedEngine("oracle_thin".to_string())
        );
    }

    #[test]
    fn dbeaver_distinguishes_mariadb_by_its_driver_not_its_provider() {
        let scan = parse_dbeaver(DBEAVER);
        let legacy = row(&scan, "Legacy");
        assert_eq!(legacy.db_type, "MariaDB");
        assert_eq!(legacy.host, "old.example");
    }

    #[test]
    fn dbeaver_carries_the_ssh_tunnel_across() {
        let scan = parse_dbeaver(DBEAVER);
        let ssh = &row(&scan, "Shop (prod)").ssh;
        assert!(ssh.enabled);
        assert_eq!(ssh.host, "bastion.example");
        assert_eq!(ssh.port, 2222);
        assert_eq!(ssh.user, "jump");
        assert_eq!(ssh.auth, SshAuth::KeyPair);
        assert_eq!(ssh.key_path, "/home/me/.ssh/id_ed25519");
        // The tunnel's own password is in the encrypted sidecar, not here.
        assert!(ssh.password.is_empty());
    }

    #[test]
    fn dbeaver_carries_the_ssl_handler_across() {
        let scan = parse_dbeaver(DBEAVER);
        let tls = &row(&scan, "Analytics").tls;
        assert_eq!(tls.mode, SslMode::VerifyCa);
        assert_eq!(tls.ca_path, "/ca.pem");
    }

    #[test]
    fn dbeavers_sqlite_database_field_is_a_file_not_a_schema() {
        let scan = parse_dbeaver(DBEAVER);
        let notes = row(&scan, "Notes");
        assert!(is_sqlite(&notes.db_type));
        assert_eq!(notes.file, "/home/me/notes.db");
        assert!(notes.database.is_empty());
    }

    #[test]
    fn a_broken_data_sources_file_is_reported_not_panicked_on() {
        let scan = parse_dbeaver("{ not json");
        assert!(scan.found.is_empty());
        assert_eq!(scan.skipped.len(), 1);
        assert!(matches!(scan.skipped[0].reason, SkipReason::Unreadable(_)));
    }

    #[test]
    fn a_dbeaver_connection_with_no_host_is_skipped_by_name() {
        let scan = parse_dbeaver(
            r#"{"connections":{"x":{"provider":"mysql","name":"Nowhere","configuration":{}}}}"#,
        );
        assert!(scan.found.is_empty());
        assert_eq!(scan.skipped[0].name, "Nowhere");
        assert_eq!(scan.skipped[0].reason, SkipReason::NoServer);
    }

    // -- DataGrip -----------------------------------------------------------

    const DATAGRIP: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
    <project version="4">
      <component name="DataSourceManagerImpl" format="xml" multifile-model="true">
        <data-source source="LOCAL" name="sakila@localhost" uuid="1-1">
          <driver-ref>mysql.8</driver-ref>
          <jdbc-driver>com.mysql.cj.jdbc.Driver</jdbc-driver>
          <jdbc-url>jdbc:mysql://localhost:3306/sakila</jdbc-url>
          <user-name>root</user-name>
        </data-source>
        <data-source source="LOCAL" name="world &amp; friends" uuid="1-2">
          <driver-ref>postgresql</driver-ref>
          <jdbc-url>jdbc:postgresql://pg.example:5432/world?sslmode=require</jdbc-url>
          <user-name>schemaic</user-name>
        </data-source>
        <data-source source="LOCAL" name="Warehouse" uuid="1-3">
          <driver-ref>oracle.16</driver-ref>
          <jdbc-url>jdbc:oracle:thin:@//ora.example:1521/wh</jdbc-url>
        </data-source>
        <data-source source="LOCAL" name="Project db" uuid="1-4">
          <driver-ref>sqlite.3</driver-ref>
          <jdbc-url>jdbc:sqlite:$PROJECT_DIR$/db/dev.sqlite</jdbc-url>
        </data-source>
      </component>
    </project>"#;

    #[test]
    fn datagrip_reads_name_url_and_user_out_of_each_data_source() {
        let scan = parse_datagrip(DATAGRIP);
        assert_eq!(scan.found.len(), 3);
        let first = &scan.found[0].connection;
        assert_eq!(first.name, "sakila@localhost");
        assert_eq!(first.host, "localhost");
        assert_eq!(first.port, 3306);
        assert_eq!(first.database, "sakila");
        assert_eq!(first.user, "root");
    }

    #[test]
    fn datagrip_names_are_xml_unescaped() {
        let scan = parse_datagrip(DATAGRIP);
        assert_eq!(scan.found[1].connection.name, "world & friends");
        assert_eq!(scan.found[1].connection.tls.mode, SslMode::Require);
    }

    #[test]
    fn datagrip_reports_an_unsupported_driver_by_its_own_name() {
        let scan = parse_datagrip(DATAGRIP);
        assert_eq!(scan.skipped.len(), 1);
        assert_eq!(scan.skipped[0].name, "Warehouse");
        assert_eq!(
            scan.skipped[0].reason,
            SkipReason::UnsupportedEngine("oracle.16".to_string())
        );
    }

    #[test]
    fn an_unexpanded_project_macro_is_flagged_rather_than_guessed_at() {
        let scan = parse_datagrip(DATAGRIP);
        let proj = scan
            .found
            .iter()
            .find(|i| i.connection.name == "Project db")
            .expect("sqlite row");
        assert!(proj.has(ImportNote::UnexpandedPath));
        assert!(proj.connection.file.contains("$PROJECT_DIR$"));
    }

    #[test]
    fn the_element_scan_does_not_confuse_a_longer_tag_for_the_one_it_wants() {
        // `<data-sources>` shares a prefix with `<data-source`.
        let xml = r#"<data-sources><data-source name="a"><jdbc-url>mysql://h/d</jdbc-url></data-source></data-sources>"#;
        let scan = parse_datagrip(xml);
        assert_eq!(scan.found.len(), 1);
        assert_eq!(scan.found[0].connection.name, "a");
    }

    #[test]
    fn an_attribute_is_not_matched_inside_a_longer_attribute_name() {
        // `uuid-name="x"` must not answer a lookup for `name`.
        let head = r#" source="LOCAL" uuid-name="x" name="real""#;
        assert_eq!(attribute(head, "name").as_deref(), Some("real"));
    }

    #[test]
    fn an_unquoted_attribute_does_not_stop_the_scan() {
        // The scanner delimits on quotes; one it cannot read is a reason to keep
        // looking, not to answer `None` for the whole head.
        let head = r#" name=bare name="real""#;
        assert_eq!(attribute(head, "name").as_deref(), Some("real"));
    }

    #[test]
    fn a_data_source_without_a_url_is_skipped_not_imported() {
        let xml = r#"<data-source name="Empty"><driver-ref>mysql.8</driver-ref></data-source>"#;
        let scan = parse_datagrip(xml);
        assert!(scan.found.is_empty());
        assert_eq!(scan.skipped[0].reason, SkipReason::NoServer);
    }

    #[test]
    fn xml_entities_including_numeric_ones_are_decoded() {
        assert_eq!(xml_unescape("a &amp; b"), "a & b");
        assert_eq!(xml_unescape("&lt;tag&gt;"), "<tag>");
        assert_eq!(xml_unescape("&quot;q&apos;"), "\"q'");
        assert_eq!(xml_unescape("&#65;&#x42;"), "AB");
        // A lone ampersand is data, not a broken entity.
        assert_eq!(xml_unescape("a & b"), "a & b");
    }

    // -- .my.cnf ------------------------------------------------------------

    #[test]
    fn my_cnf_reads_the_client_group() {
        let scan = parse_my_cnf(
            "[client]\nhost = db.example\nport = 3307\nuser = app\npassword = \"s3cret\"\ndatabase=shop\n",
        );
        assert_eq!(scan.found.len(), 1);
        let c = &scan.found[0].connection;
        assert_eq!(c.host, "db.example");
        assert_eq!(c.port, 3307);
        assert_eq!(c.user, "app");
        assert_eq!(c.password, "s3cret");
        assert_eq!(c.database, "shop");
        assert_eq!(c.name, "shop@db.example");
    }

    #[test]
    fn my_cnf_with_no_host_means_localhost() {
        let scan = parse_my_cnf("[client]\nuser=root\npassword=pw\n");
        assert_eq!(scan.found[0].connection.host, "127.0.0.1");
        assert_eq!(scan.found[0].connection.port, 3306);
    }

    #[test]
    fn a_suffixed_client_group_is_layered_on_the_base_one() {
        let scan = parse_my_cnf(
            "[client]\nhost=shared.example\nuser=app\npassword=base\n\n[client_prod]\nhost=prod.example\npassword=prodpw\n",
        );
        let prod = scan
            .found
            .iter()
            .find(|i| i.connection.name == "client_prod")
            .expect("suffixed group");
        assert_eq!(prod.connection.host, "prod.example");
        assert_eq!(prod.connection.password, "prodpw");
        // Inherited from [client], which is what --defaults-group-suffix does.
        assert_eq!(prod.connection.user, "app");
    }

    #[test]
    fn the_mysqld_group_is_not_a_client() {
        let scan = parse_my_cnf("[mysqld]\nuser=mysql\nport=3306\ndatadir=/var/lib/mysql\n");
        assert!(scan.found.is_empty());
    }

    #[test]
    fn my_cnf_underscores_and_dashes_name_one_key() {
        let a = parse_my_cnf("[client]\nhost=h\nssl_mode=REQUIRED\n");
        let b = parse_my_cnf("[client]\nhost=h\nssl-mode=REQUIRED\n");
        assert_eq!(a.found[0].connection.tls.mode, SslMode::Require);
        assert_eq!(b.found[0].connection.tls.mode, SslMode::Require);
    }

    #[test]
    fn an_include_directive_is_ignored_rather_than_read_as_a_key() {
        let scan = parse_my_cnf("!includedir /etc/mysql/conf.d/\n[client]\nhost=h\nuser=u\n");
        assert_eq!(scan.found.len(), 1);
        assert_eq!(scan.found[0].connection.host, "h");
    }

    /// A BOM is what Notepad writes, and `str::trim` does not remove it — so
    /// `<BOM>[client]` was read as a bare key in the unnamed section and the
    /// whole file imported **zero rows with zero skipped entries**, which the
    /// modal renders as "nothing to import" for a file full of servers.
    ///
    /// Asserted through the parsers rather than over `strip_bom`, because that
    /// helper was already right in `import.rs` while all four of these were
    /// wrong.
    #[test]
    fn a_byte_order_mark_does_not_empty_a_client_config_file() {
        let scan = parse_my_cnf("\u{feff}[client]\nhost=db.example\nuser=app\npassword=pw\n");
        assert_eq!(scan.found.len(), 1, "{:?}", scan.skipped);
        assert_eq!(scan.found[0].connection.host, "db.example");

        let scan = parse_pg_service("\u{feff}[prod]\nhost=pg.example\nuser=app\n");
        assert_eq!(scan.found.len(), 1, "{:?}", scan.skipped);
        assert_eq!(scan.found[0].connection.host, "pg.example");
    }

    /// The `.pgpass` half is quieter and worse: the file parses, but the first
    /// line's host carries the BOM, so it matches no connection and the real
    /// password sits unused while every row keeps its "no password" note.
    #[test]
    fn a_byte_order_mark_does_not_give_pgpass_a_host_that_can_never_match() {
        let entries = pgpass_entries("\u{feff}db.example:5432:shop:app:hunter2\n");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].host, "db.example");
    }

    /// And the two JSON/XML sources, where the BOM makes `serde_json` refuse
    /// the whole file with a parse error the user cannot act on.
    #[test]
    fn a_byte_order_mark_does_not_make_a_client_export_unreadable() {
        let scan = parse_dbeaver(&format!("\u{feff}{DBEAVER}"));
        assert!(!scan.found.is_empty(), "{:?}", scan.skipped);

        let scan = parse_datagrip(&format!("\u{feff}{DATAGRIP}"));
        assert!(!scan.found.is_empty(), "{:?}", scan.skipped);
    }

    /// The paste field takes one too — copying a URL out of a Windows tool can
    /// carry it.
    #[test]
    fn a_byte_order_mark_does_not_stop_a_pasted_url() {
        assert_eq!(url("\u{feff}mysql://h/d").host, "h");
    }

    #[test]
    fn identical_groups_collapse_to_one_row() {
        // `[mysql]` inheriting `[client]` entirely is the same connection twice.
        let scan = parse_my_cnf("[client]\nhost=h\nuser=u\npassword=p\n[mysql]\n");
        assert_eq!(scan.found.len(), 1);
    }

    // -- .pgpass and .pg_service.conf ---------------------------------------

    #[test]
    fn pgpass_lines_parse_with_their_escapes() {
        let e = pgpass_entries("db.example:5432:world:schemaic:p\\:ss\n# comment\n\n");
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].host, "db.example");
        assert_eq!(e[0].port, "5432");
        assert_eq!(e[0].database, "world");
        assert_eq!(e[0].user, "schemaic");
        assert_eq!(e[0].password, "p:ss");
    }

    #[test]
    fn a_password_containing_a_colon_is_not_split_further() {
        let e = pgpass_entries("h:5432:d:u:a:b:c");
        assert_eq!(e[0].password, "a:b:c");
    }

    #[test]
    fn a_wildcard_host_names_no_server_so_it_is_skipped() {
        let scan = parse_pgpass("*:*:*:postgres:secret\nreal.example:5432:world:me:pw\n");
        assert_eq!(scan.found.len(), 1);
        assert_eq!(scan.found[0].connection.host, "real.example");
        assert_eq!(scan.skipped.len(), 1);
        assert_eq!(scan.skipped[0].reason, SkipReason::NoServer);
    }

    #[test]
    fn a_pgpass_wildcard_still_completes_another_sources_row() {
        let mut found = vec![imported(
            {
                let mut c = blank(POSTGRES);
                c.host = "pg.example".into();
                c.port = 5432;
                c.database = "world".into();
                c.user = "schemaic".into();
                c
            },
            ImportSource::DataGrip,
        )];
        fill_missing_passwords(&mut found, &pgpass_entries("*:*:*:schemaic:secret\n"));
        assert_eq!(found[0].connection.password, "secret");
    }

    #[test]
    fn a_row_with_no_user_adopts_the_pgpass_line_it_matched() {
        let mut found = vec![imported(
            {
                let mut c = blank(POSTGRES);
                c.host = "pg.example".into();
                c.database = "world".into();
                c
            },
            ImportSource::DBeaver,
        )];
        fill_missing_passwords(
            &mut found,
            &pgpass_entries("pg.example:5432:world:owner:pw\n"),
        );
        assert_eq!(found[0].connection.user, "owner");
        assert_eq!(found[0].connection.password, "pw");
    }

    #[test]
    fn pgpass_never_completes_a_row_on_another_engine() {
        let mut found = vec![imported(
            {
                let mut c = blank(MYSQL);
                c.host = "pg.example".into();
                c.user = "schemaic".into();
                c
            },
            ImportSource::DataGrip,
        )];
        fill_missing_passwords(&mut found, &pgpass_entries("*:*:*:*:secret\n"));
        assert!(found[0].connection.password.is_empty());
    }

    #[test]
    fn filling_a_password_retracts_the_note_saying_there_is_none() {
        // The order inside `scan` hides this; a caller completing an
        // already-scanned row (a hand-picked file) is where it shows.
        let mut row = imported(
            {
                let mut c = blank(POSTGRES);
                c.host = "pg.example".into();
                c.user = "schemaic".into();
                c
            },
            ImportSource::DataGrip,
        );
        row.note(ImportNote::NoPassword);
        let mut found = vec![row];
        fill_missing_passwords(&mut found, &pgpass_entries("*:*:*:schemaic:secret\n"));
        assert_eq!(found[0].connection.password, "secret");
        assert!(
            !found[0].has(ImportNote::NoPassword),
            "the row has one now, and must stop saying otherwise"
        );
    }

    #[test]
    fn a_row_no_pgpass_line_matches_keeps_its_note() {
        let mut row = imported(
            {
                let mut c = blank(POSTGRES);
                c.host = "pg.example".into();
                c.user = "schemaic".into();
                c
            },
            ImportSource::DataGrip,
        );
        row.note(ImportNote::NoPassword);
        let mut found = vec![row];
        fill_missing_passwords(&mut found, &pgpass_entries("other.example:5432:d:u:pw\n"));
        assert!(found[0].connection.password.is_empty());
        assert!(found[0].has(ImportNote::NoPassword));
    }

    #[test]
    fn pgpass_leaves_a_password_that_is_already_there_alone() {
        let mut found = vec![imported(
            {
                let mut c = blank(POSTGRES);
                c.host = "pg.example".into();
                c.user = "u".into();
                c.password = "fromurl".into();
                c
            },
            ImportSource::Url,
        )];
        fill_missing_passwords(&mut found, &pgpass_entries("*:*:*:*:frompgpass\n"));
        assert_eq!(found[0].connection.password, "fromurl");
    }

    #[test]
    fn pg_service_sections_are_named_connections() {
        let scan = parse_pg_service(
            "# comment\n[analytics]\nhost=pg.example\nport=5433\ndbname=warehouse\nuser=ro\nsslmode=verify-full\n\n[nohost]\ndbname=x\n",
        );
        assert_eq!(scan.found.len(), 1);
        let c = &scan.found[0].connection;
        assert_eq!(c.name, "analytics");
        assert_eq!(c.host, "pg.example");
        assert_eq!(c.port, 5433);
        assert_eq!(c.database, "warehouse");
        assert_eq!(c.user, "ro");
        assert_eq!(c.tls.mode, SslMode::VerifyFull);
        assert_eq!(scan.skipped[0].name, "nohost");
    }

    // -- identity -----------------------------------------------------------

    fn at(host: &str, port: u16, db: &str, user: &str) -> Connection {
        let mut c = blank(MYSQL);
        c.host = host.into();
        c.port = port;
        c.database = db.into();
        c.user = user.into();
        c
    }

    #[test]
    fn the_same_server_under_a_different_name_is_the_same_endpoint() {
        let mut a = at("h", 3306, "d", "u");
        a.name = "From DBeaver".into();
        let mut b = at("H", 3306, "D", "u");
        b.name = "From DataGrip".into();
        assert!(same_endpoint(&a, &b));
    }

    #[test]
    fn a_different_login_on_one_server_is_a_different_connection() {
        assert!(!same_endpoint(
            &at("h", 3306, "d", "owner"),
            &at("h", 3306, "d", "readonly")
        ));
    }

    #[test]
    fn two_engines_on_one_host_and_port_are_not_the_same_endpoint() {
        let mut pg = at("h", 3306, "d", "u");
        pg.db_type = POSTGRES.into();
        assert!(!same_endpoint(&at("h", 3306, "d", "u"), &pg));
    }

    #[test]
    fn sqlite_identity_is_the_file_and_separators_do_not_change_it() {
        let mut a = blank(SQLITE);
        a.file = r"C:\data\app.db".into();
        let mut b = blank(SQLITE);
        b.file = "C:/data/app.db".into();
        assert!(same_endpoint(&a, &b));
    }

    #[test]
    fn dedupe_keeps_the_first_description_of_a_server() {
        let mut first = imported(at("h", 3306, "d", "u"), ImportSource::DBeaver);
        first.connection.name = "Shop (prod)".into();
        let mut second = imported(at("h", 3306, "d", "u"), ImportSource::MyCnf);
        second.connection.name = "d@h".into();
        let out = dedupe(vec![first, second]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].connection.name, "Shop (prod)");
    }

    #[test]
    fn merging_appends_new_rows_and_ticks_them() {
        let mut list = vec![imported(at("a", 3306, "d", "u"), ImportSource::DBeaver)];
        let tick = merge_rows(
            &mut list,
            vec![
                imported(at("b", 3306, "d", "u"), ImportSource::Url),
                imported(at("c", 3306, "d", "u"), ImportSource::Url),
            ],
        );
        assert_eq!(list.len(), 3);
        assert_eq!(tick, [1, 2], "the indices of what was appended");
    }

    #[test]
    fn merging_a_repeat_selects_the_row_already_there() {
        // Pasting a URL for a server the scan already found must do something
        // visible, and a second row is not it.
        let mut list = vec![imported(at("a", 3306, "d", "u"), ImportSource::DBeaver)];
        let tick = merge_rows(
            &mut list,
            vec![imported(at("A", 3306, "D", "u"), ImportSource::Url)],
        );
        assert_eq!(list.len(), 1, "no second row for one server");
        assert_eq!(tick, [0]);
        assert_eq!(list[0].source, ImportSource::DBeaver, "the first survives");
    }

    #[test]
    fn merging_never_ticks_a_row_that_is_already_saved() {
        let mut already = imported(at("a", 3306, "d", "u"), ImportSource::DataGrip);
        already.note(ImportNote::AlreadySaved);
        let mut list: Vec<Imported> = Vec::new();
        assert!(
            merge_rows(&mut list, vec![already]).is_empty(),
            "appended, but not ticked"
        );
        assert_eq!(list.len(), 1);
        // And the same when the duplicate is the row *already* on the list: the
        // answer must not depend on which side of the merge it arrived on.
        let tick = merge_rows(
            &mut list,
            vec![imported(at("a", 3306, "d", "u"), ImportSource::Url)],
        );
        assert!(tick.is_empty());
    }

    #[test]
    fn merging_keeps_earlier_indices_valid() {
        // The UI keys its selection by index into this list, so an append must
        // never move a row that is already in it.
        let mut list = vec![
            imported(at("a", 3306, "d", "u"), ImportSource::DBeaver),
            imported(at("b", 3306, "d", "u"), ImportSource::DBeaver),
        ];
        let before: Vec<String> = list.iter().map(|i| i.connection.host.clone()).collect();
        merge_rows(
            &mut list,
            vec![imported(at("c", 3306, "d", "u"), ImportSource::Url)],
        );
        assert_eq!(
            list[..2]
                .iter()
                .map(|i| i.connection.host.clone())
                .collect::<Vec<_>>(),
            before
        );
    }

    #[test]
    fn skipped_entries_do_not_pile_up_when_a_source_is_read_twice() {
        // The bug this exists for: scanning twice turned three Oracle data
        // sources into "6 entries were not imported", naming each one twice.
        let one = || Skipped {
            name: "Warehouse".to_string(),
            reason: SkipReason::UnsupportedEngine("oracle.16".to_string()),
        };
        let mut list = Vec::new();
        merge_skipped(&mut list, vec![one()]);
        merge_skipped(&mut list, vec![one()]);
        assert_eq!(list.len(), 1);
    }

    #[test]
    fn merging_skipped_keeps_two_entries_that_only_look_alike() {
        // Same name, different reason — two real facts about two entries.
        let mut list = Vec::new();
        merge_skipped(
            &mut list,
            vec![
                Skipped {
                    name: "Warehouse".to_string(),
                    reason: SkipReason::UnsupportedEngine("oracle.16".to_string()),
                },
                Skipped {
                    name: "Warehouse".to_string(),
                    reason: SkipReason::NoServer,
                },
            ],
        );
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn a_connection_the_user_already_has_is_marked_and_not_preselected() {
        let mut saved = at("h", 3306, "d", "u");
        saved.id = 7;
        saved.name = "Mine".into();
        let mut found = vec![imported(at("h", 3306, "d", "u"), ImportSource::DataGrip)];
        mark_existing(&mut found, &[saved]);
        assert!(found[0].has(ImportNote::AlreadySaved));
        assert!(!found[0].preselected());
    }

    // -- scan ---------------------------------------------------------------

    fn file(source: ImportSource, path: &str, text: &str) -> SourceFile {
        SourceFile {
            source,
            path: path.to_string(),
            text: text.to_string(),
        }
    }

    #[test]
    fn scan_completes_a_datagrip_row_from_pgpass_before_deduping_it() {
        // The whole point of the ordering: DataGrip knows the server and its
        // name, .pgpass knows the password, and the row that survives must have
        // both. Parsed second, so a naive dedupe would drop the password.
        let files = [
            file(
                ImportSource::DataGrip,
                "/x/dataSources.xml",
                r#"<data-source name="World"><jdbc-url>jdbc:postgresql://pg.example:5432/world</jdbc-url><user-name>schemaic</user-name></data-source>"#,
            ),
            file(
                ImportSource::Pgpass,
                "/home/me/.pgpass",
                "pg.example:5432:world:schemaic:secret\n",
            ),
        ];
        let scan = scan(&files, &[]);
        assert_eq!(scan.found.len(), 1, "the two describe one server");
        let row = &scan.found[0];
        assert_eq!(row.connection.name, "World");
        assert_eq!(row.connection.password, "secret");
        assert_eq!(row.source, ImportSource::DataGrip);
        assert_eq!(row.origin, "/x/dataSources.xml");
        assert!(!row.has(ImportNote::NoPassword));
    }

    #[test]
    fn scan_flags_the_rows_no_source_could_give_a_password() {
        let files = [file(
            ImportSource::DataGrip,
            "/x/dataSources.xml",
            r#"<data-source name="Shop"><jdbc-url>jdbc:mysql://h:3306/shop</jdbc-url><user-name>app</user-name></data-source>"#,
        )];
        let scan = scan(&files, &[]);
        assert!(scan.found[0].has(ImportNote::NoPassword));
        assert!(scan.found[0].preselected(), "still worth importing");
    }

    #[test]
    fn a_connection_with_no_user_is_not_flagged_for_a_missing_password() {
        let files = [file(
            ImportSource::Url,
            "",
            "sqlite:///var/db/app.db\nmysql://h/d\n",
        )];
        let scan = scan(&files, &[]);
        assert_eq!(scan.found.len(), 2);
        assert!(
            !scan.found[0].has(ImportNote::NoPassword),
            "sqlite has none"
        );
        assert!(!scan.found[1].has(ImportNote::NoPassword), "no user named");
    }

    #[test]
    fn scan_marks_against_what_is_already_saved() {
        let mut saved = blank(MYSQL);
        saved.id = 3;
        saved.host = "h".into();
        saved.database = "d".into();
        saved.user = "u".into();
        let files = [file(ImportSource::Url, "", "mysql://u@h/d")];
        let scan = scan(&files, &[saved]);
        assert!(scan.found[0].has(ImportNote::AlreadySaved));
    }

    #[test]
    fn scan_carries_a_files_path_onto_every_row_it_produced() {
        let files = [file(
            ImportSource::MyCnf,
            "/home/me/.my.cnf",
            "[client]\nhost=h\nuser=u\n",
        )];
        let scan = scan(&files, &[]);
        assert_eq!(scan.found[0].origin, "/home/me/.my.cnf");
    }

    #[test]
    fn every_imported_connection_arrives_without_an_identity() {
        // The app assigns the id; a non-zero one here would collide with a saved
        // connection's keyring slot.
        let files = [
            file(ImportSource::Url, "", "mysql://u:p@h/d"),
            file(ImportSource::DBeaver, "/x.json", DBEAVER),
            file(ImportSource::DataGrip, "/x.xml", DATAGRIP),
        ];
        let scan = scan(&files, &[]);
        assert!(!scan.found.is_empty());
        assert!(scan.found.iter().all(|i| i.connection.id == 0));
    }
}
