//! Which servers the live tier runs against, and how it is told about them.
//!
//! One environment variable per field rather than a DSN, for the reason
//! `examples/tls_matrix.rs` uses the same shape: there is no URL parser here to
//! get wrong, and a password containing `@ / # ? % :` needs no encoding. Every
//! field has a default pointing at this project's own test bed, so a developer
//! with those servers running types nothing.
//!
//! | variable | default |
//! |---|---|
//! | `SCHEMAIC_IT_MARIADB_HOST` / `_PORT` / `_USER` / `_PASSWORD` | `127.0.0.1` / `3306` / `schemaic` / `schemaic` |
//! | `SCHEMAIC_IT_MYSQL_HOST` / `_PORT` / `_USER` / `_PASSWORD` | `127.0.0.1` / `3307` / `schemaic` / `schemaic` |
//! | `SCHEMAIC_IT_PG_HOST` / `_PORT` / `_USER` / `_PASSWORD` | `127.0.0.1` / `5432` / `schemaic` / `schemaic` |
//! | `SCHEMAIC_IT_ENGINES` | unset — every leg runs |
//!
//! **A missing server is a failure, not a skip.** The tier is off entirely
//! unless `--features live-tests` is passed, so anyone who has turned it on has
//! said they have the servers; a harness that noticed an unreachable endpoint
//! and returned would report a green suite that asserted nothing.
//! `SCHEMAIC_IT_ENGINES` is the one way to run less than everything, and it
//! costs a developer a deliberate sentence: `SCHEMAIC_IT_ENGINES=mariadb,pg`.

use schemaic_db::{Db, Engine};

use crate::cases::{self, TypeCase};

/// One server the suite runs against.
///
/// **MariaDB is its own leg, not a MySQL variant.** Both speak
/// [`Engine::MySql`] to this crate, and they diverge in exactly the places the
/// DB layer reads — `information_schema`, `CHECK` clause escaping, JSON,
/// sequences — which is how a MySQL 8 quirk in `CHECK_CLAUSE` once hid behind a
/// MariaDB that returned runnable text.
pub struct Target {
    /// What the leg is called, in test names and in `SCHEMAIC_IT_ENGINES`.
    pub name: &'static str,
    /// The driver this server is reached with.
    pub engine: Engine,
    /// Prefix of this leg's environment variables.
    env: &'static str,
    default_port: u16,
    default_user: &'static str,
    /// The namespace a table in this leg's scratch database reports — a
    /// PostgreSQL schema, or `None` where a database *is* the namespace.
    ///
    /// Data on the target rather than an `if engine == Postgres` in a test body:
    /// the suite is written once, so what differs between servers belongs in the
    /// table describing them, the same way production code asks a capability.
    pub namespace: Option<&'static str>,
    /// How this server spells a short raw-bytes column. Data on the target for
    /// the same reason `namespace` is: what differs between servers belongs in
    /// the table describing them, not in an `if` inside a test.
    pub binary_type: &'static str,
    /// The table clause that makes a table **non-transactional**, where this
    /// server has one — MySQL's `MyISAM`, which accepts `BEGIN`/`ROLLBACK` and
    /// ignores them. `None` on PostgreSQL, which has no such table.
    ///
    /// It is here so `a_failed_batch_says_what_the_rollback_actually_undid` can
    /// assert both halves of [`schemaic_core::model::Rollback`] where both
    /// exist, and the honest half where only one does — rather than skipping.
    pub non_transactional: Option<&'static str>,
    /// Does a **DDL** plan roll back as a whole on this server?
    ///
    /// PostgreSQL's `run_ddl` wraps the plan in `BEGIN`/`ROLLBACK` and its DDL
    /// honours it, so a refused plan applies *nothing*. MySQL and MariaDB commit
    /// each `ALTER` as it runs, so the statements before the failure are on the
    /// table for good — which is why `DdlError::applied` exists and why the
    /// preview's failure message counts it.
    pub transactional_ddl: bool,
    /// The types this server is asked to round-trip, and the ones only it has.
    /// Two slices rather than one so MySQL and MariaDB can share the twenty they
    /// agree on and still each own the one they do not — see [`cases`].
    types: &'static [TypeCase],
    extra_types: &'static [TypeCase],
}

pub static MARIADB: Target = Target {
    name: "mariadb",
    engine: Engine::MySql,
    env: "SCHEMAIC_IT_MARIADB",
    default_port: 3306,
    default_user: "schemaic",
    namespace: None,
    binary_type: "VARBINARY(4)",
    non_transactional: Some("ENGINE=MyISAM"),
    transactional_ddl: false,
    types: cases::MYSQL_FAMILY,
    extra_types: cases::MARIADB_ONLY,
};

pub static MYSQL: Target = Target {
    name: "mysql",
    engine: Engine::MySql,
    env: "SCHEMAIC_IT_MYSQL",
    default_port: 3307,
    default_user: "schemaic",
    namespace: None,
    binary_type: "VARBINARY(4)",
    non_transactional: Some("ENGINE=MyISAM"),
    transactional_ddl: false,
    types: cases::MYSQL_FAMILY,
    extra_types: cases::MYSQL_ONLY,
};

pub static POSTGRES: Target = Target {
    name: "pg",
    engine: Engine::Postgres,
    env: "SCHEMAIC_IT_PG",
    default_port: 5432,
    default_user: "schemaic",
    namespace: Some("public"),
    binary_type: "bytea",
    non_transactional: None,
    transactional_ddl: true,
    types: cases::POSTGRES,
    extra_types: &[],
};

/// Every leg, in the order the suite reports them.
pub static ALL: &[&Target] = &[&MARIADB, &MYSQL, &POSTGRES];

impl Target {
    /// A handle on this server that is attached to **no database** — the one
    /// that can create and drop a scratch database, since on MySQL the database
    /// is part of the handshake and a connection pointed at its own target
    /// cannot drop it.
    pub fn base_db(&self) -> Db {
        Db::from_parts(
            self.engine,
            self.var("HOST", "127.0.0.1"),
            self.port(),
            self.var("USER", self.default_user),
            self.var("PASSWORD", "schemaic"),
            String::new(),
        )
    }

    /// Every type case this server answers for.
    pub fn type_cases(&self) -> impl Iterator<Item = &'static TypeCase> {
        self.types.iter().chain(self.extra_types)
    }

    /// How this leg is spelled in an error, so a failure names the endpoint a
    /// developer has to go and look at rather than only the assertion.
    pub fn endpoint(&self) -> String {
        format!(
            "{} at {}:{} as {}",
            self.name,
            self.var("HOST", "127.0.0.1"),
            self.port(),
            self.var("USER", self.default_user)
        )
    }

    /// Was this leg asked for? True unless `SCHEMAIC_IT_ENGINES` is set and
    /// leaves it out.
    pub fn enabled(&self) -> bool {
        let Some(list) = engines_var() else {
            return true;
        };
        list.iter().any(|n| n == self.name)
    }

    fn port(&self) -> u16 {
        let raw = self.var("PORT", "");
        if raw.is_empty() {
            return self.default_port;
        }
        // Not a silent fall back to the default: a mistyped port that quietly
        // becomes 3306 runs the whole leg against the wrong server and passes.
        raw.parse().unwrap_or_else(|_| {
            panic!("{}_PORT is not a port number: {raw:?}", self.env);
        })
    }

    /// One field of this leg's endpoint. An empty value counts as unset, because
    /// PowerShell cannot hold one — `$env:X = ''` *removes* the variable — so the
    /// two spellings have to mean the same thing on the shell most likely to run
    /// this on Windows.
    fn var(&self, field: &str, default: &str) -> String {
        match std::env::var(format!("{}_{field}", self.env)) {
            Ok(v) if !v.is_empty() => v,
            _ => default.to_string(),
        }
    }
}

/// The parsed `SCHEMAIC_IT_ENGINES`, or `None` when every leg runs.
///
/// An unrecognised name is refused rather than ignored: `SCHEMAIC_IT_ENGINES=postgres`
/// (the engine's name, not the leg's) would otherwise disable all three and
/// report a suite that ran nothing at all.
fn engines_var() -> Option<Vec<String>> {
    let raw = std::env::var("SCHEMAIC_IT_ENGINES").ok()?;
    if raw.trim().is_empty() {
        return None;
    }
    // **Not in CI.** libtest has no runtime skip, so a leg left out reports as a
    // passing test that asserted nothing — tolerable when a developer typed the
    // exclusion on their own machine and can see it on stderr, and exactly the
    // silent green this tier exists to avoid when it is a workflow file nobody
    // rereads. The variable is a local convenience; there it is a mistake.
    assert!(
        std::env::var_os("CI").is_none(),
        "SCHEMAIC_IT_ENGINES is set to {raw:?} in CI, where every leg must run — \
         a leg left out reports as a pass having asserted nothing"
    );
    let names: Vec<String> = raw
        .split(',')
        .map(|n| n.trim().to_ascii_lowercase())
        .filter(|n| !n.is_empty())
        .collect();
    for n in &names {
        assert!(
            ALL.iter().any(|t| t.name == n),
            "SCHEMAIC_IT_ENGINES names {n:?}, which is not a leg — valid names are {}",
            ALL.iter().map(|t| t.name).collect::<Vec<_>>().join(", ")
        );
    }
    Some(names)
}
