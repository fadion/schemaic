//! A scratch database per test, created and dropped around it.
//!
//! **Nothing in this tier ever touches a database it did not create.** Every
//! name is generated here and carries [`PREFIX`]; both the create and the drop
//! path assert it, so the statement that says `DROP DATABASE` cannot be reached
//! with a name that came from anywhere else. That guard is what stands between a
//! live suite and the README's warning about pointing this app at data you care
//! about, and it is cheaper than the afternoon spent proving a dropped database
//! was not somebody's.
//!
//! The name also carries the process id, so two runs of the suite — a local
//! shell and an editor, or two CI jobs against one server — cannot collide on a
//! namespace. A run killed hard leaves its databases behind; they are all
//! `schemaic_it_%`, so the sweep is:
//!
//! ```text
//! MySQL/MariaDB: SELECT CONCAT('DROP DATABASE `', schema_name, '`;')
//!                FROM information_schema.schemata WHERE schema_name LIKE 'schemaic\_it\_%';
//! PostgreSQL:    SELECT format('DROP DATABASE %I WITH (FORCE);', datname)
//!                FROM pg_database WHERE datname LIKE 'schemaic\_it\_%';
//! ```
//!
//! It is deliberately manual: a sweep that ran automatically would drop the
//! namespaces of a *concurrent* run, which is the one failure a fixture must not
//! introduce.

use schemaic_core::edit::{EditModel, analyze_edit};
use schemaic_core::export::ident_sql;
use schemaic_core::intel::SqlDialect;
use schemaic_core::model::ResultSet;
use schemaic_core::schema::DbSchema;
use schemaic_db::{Db, DbError};
use tokio_util::sync::CancellationToken;

use crate::endpoint::Target;

/// Every namespace this tier creates begins with this, and nothing without it is
/// ever dropped.
pub const PREFIX: &str = "schemaic_it_";

/// What [`Scratch::alt_namespace`]'s second namespace is called, on whichever
/// level this server keeps its namespaces.
const ALT: &str = "alt";

/// A place a table can live, on either shape of server: a database, plus the
/// schema inside it where the server has that level.
///
/// The pair rather than a string because it is exactly what every namespace-aware
/// API here already takes — `fetch_table`, `ImportTarget`, `RowEdit` — and
/// collapsing the two is the mistake those APIs are shaped to prevent.
#[derive(Clone, Debug)]
pub struct Namespace {
    pub database: String,
    pub schema: Option<String>,
}

/// How many rows a fixture statement may return. Fixture data is seeded by the
/// test that reads it, so anything approaching this is a bug in the test.
const ROW_CAP: usize = 10_000;

/// A database that exists for one test and is dropped when it ends.
pub struct Scratch {
    /// Attached to no database — the handle that creates and drops.
    base: Db,
    /// Scoped to the scratch database: what a test runs its statements on.
    pub db: Db,
    pub database: String,
    /// The namespace a table here reports, from [`Target::namespace`].
    pub namespace: Option<&'static str>,
    dialect: SqlDialect,
    /// Further scratch **databases** this test made — the MySQL family's answer
    /// to [`Scratch::alt_namespace`], where a database *is* the namespace. Empty
    /// on PostgreSQL, whose second namespace is a schema inside this one. Dropped
    /// before [`Scratch::database`] is.
    extra_databases: Vec<String>,
    /// Set by [`Scratch::teardown`], so the drop guard does not run twice.
    torn: bool,
}

/// Every scratch name currently held, so the uniqueness contract is **enforced**
/// rather than stated.
///
/// `test` is hand-written at 65 call sites and the suite runs threaded, so two
/// tests that pick the same label share one database — each dropping the other's
/// tables, with failures that read as anything but a name collision. Nothing
/// checked it: a duplicate reaches `CREATE DATABASE` as an error only if the two
/// happen to overlap, which is precisely the race that does not reproduce.
///
/// Keyed on the *whole* name, so the same label on two legs is fine (the leg is
/// part of it), and released on teardown so a test that creates, tears down and
/// creates again is not refused.
/// How long a scratch teardown may take before it is reported as a leftover.
///
/// Generous against a slow server and far short of hanging the binary: a
/// `DROP DATABASE` waits on an open transaction, which is a state this tier
/// deliberately creates.
const TEARDOWN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

fn live_names() -> &'static std::sync::Mutex<std::collections::HashSet<String>> {
    static NAMES: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<String>>> =
        std::sync::OnceLock::new();
    NAMES.get_or_init(Default::default)
}

fn claim_name(name: &str) {
    let mut held = live_names().lock().expect("the name set is not poisoned");
    assert!(
        held.insert(name.to_string()),
        "two live tests are using the scratch name {name:?} at once — \
         `Scratch::create`'s label must be unique within the binary"
    );
}

fn release_name(name: &str) {
    if let Ok(mut held) = live_names().lock() {
        held.remove(name);
    }
}

impl Scratch {
    /// Create a scratch database for `test`, and hand back a handle scoped to it.
    ///
    /// `test` names the test and must be unique within the binary — the same
    /// contract `sqlite::tests::shared_memory` has, and for the same reason: the
    /// suite runs threaded and the name is the whole identity. Enforced by
    /// [`claim_name`], not left to 65 call sites.
    pub async fn create(target: &'static Target, test: &str) -> Scratch {
        // The leg is part of the name as well as the process id: the suite is
        // one binary running every server at once, so two legs pointed at the
        // same host — a misconfiguration, but a plausible one — would otherwise
        // race for one namespace. It also makes a leftover self-identifying.
        let name = format!("{PREFIX}{}_{}_{test}", std::process::id(), target.name);
        assert_scratch_name(&name);
        claim_name(&name);
        // 64 bytes on MySQL, 63 on PostgreSQL. Caught here rather than as a
        // truncated name that two tests then share.
        assert!(
            name.len() <= 63,
            "scratch name {name:?} is {} bytes; shorten the test name",
            name.len()
        );

        let base = target.base_db();
        let dialect = target.engine.dialect();
        // Reached through the unscoped handle: on MySQL the database is part of
        // the handshake, so a CREATE DATABASE issued from a connection pointed at
        // a database that does not exist yet never gets to run.
        let sql = format!("CREATE DATABASE {}", ident_sql(&name, dialect));
        base.fetch_query(None, &sql, 1, CancellationToken::new())
            .await
            .unwrap_or_else(|e| {
                panic!(
                    "live tier could not create a scratch database on {}: {e}\nstatement: {sql}",
                    target.endpoint()
                )
            });

        let db = base.clone().with_database(Some(&name));
        Scratch {
            base,
            db,
            database: name,
            namespace: target.namespace,
            dialect,
            extra_databases: Vec::new(),
            torn: false,
        }
    }

    /// Run one statement against the scratch database and return its result.
    /// Any failure panics with the statement, since a fixture that half-applied
    /// makes every assertion after it meaningless.
    pub async fn exec(&self, sql: &str) -> ResultSet {
        self.try_exec(sql)
            .await
            .unwrap_or_else(|e| panic!("live tier statement failed: {e}\nstatement: {sql}"))
    }

    /// The same, handing the failure back instead of panicking — for a matrix
    /// that wants to report every case that failed rather than stop at the
    /// first, which is the difference between one round of fixing and twenty.
    pub async fn try_exec(&self, sql: &str) -> Result<ResultSet, DbError> {
        self.db
            .fetch_query(Some(&self.database), sql, ROW_CAP, CancellationToken::new())
            .await
    }

    /// The dialect this server's literals and identifiers are written in.
    pub fn dialect(&self) -> SqlDialect {
        self.dialect
    }

    /// The result of `sql`, and the edit model the app would build from it.
    ///
    /// **The composition is the point.** `edit::analyze_edit` has unit tests, and
    /// every one of them hands it a `ColumnOrigin` written out by hand — so what
    /// they prove is that the ladder works on the metadata a test *imagined*.
    /// Whether a real driver reports `org_table` for an alias, a `table_oid` for
    /// a joined column or a primary-key flag at all is decided on the wire, and
    /// the two halves have never met outside the running app.
    ///
    /// The schema is fetched per call rather than cached, because that is also
    /// what the app does: `analyze_edit`'s `schema_for` reads whatever the tree
    /// last loaded.
    pub async fn edit_model(&self, sql: &str) -> (ResultSet, EditModel) {
        let rs = self.exec(sql).await;

        // **Every database the result names, not just this one.** The lookup
        // `analyze_edit` is given takes a database, a namespace and a table, and
        // a helper that answered from one database while ignoring the first
        // argument would hand `alt.orders` the *primary* namespace's `orders` —
        // collapsing precisely the identity the namespace tests exist to check,
        // inside the helper those tests call.
        let mut databases: Vec<String> = rs
            .columns
            .iter()
            .filter_map(|c| c.origin.as_ref().map(|o| o.database.clone()))
            .collect();
        databases.sort();
        databases.dedup();
        if databases.is_empty() {
            databases.push(self.database.clone());
        }

        let mut loaded: Vec<(String, DbSchema)> = Vec::new();
        for db in databases {
            let schema = self
                .db
                .clone()
                .with_database(Some(&db))
                .fetch_schema(&db, CancellationToken::new())
                .await
                .unwrap_or_else(|e| panic!("live tier could not introspect {db}: {e}"));
            loaded.push((db, schema));
        }

        let model = analyze_edit(&rs, |db, ns, table| {
            loaded
                .iter()
                .find(|(name, _)| name == db)
                .and_then(|(_, schema)| {
                    schema
                        .tables
                        .iter()
                        .find(|t| t.name == table && t.schema.as_deref() == ns)
                })
                .cloned()
        });
        (rs, model)
    }

    /// A **second** namespace, whatever that means on this server.
    ///
    /// PostgreSQL has a level between database and table, so this is a schema
    /// beside `public` in the same database. MySQL has none — a database *is* the
    /// namespace — so it is a second scratch database, carrying the same prefix
    /// and dropped by the same guard. Returned as the pair every namespace-aware
    /// API takes, so a test can be written once for both.
    ///
    /// It exists because "two namespaces may hold same-named tables" is a rule
    /// about *identity*, and the failure it guards against — one namespace's key
    /// addressing another's rows — cannot happen with only one namespace to
    /// test in.
    pub async fn alt_namespace(&mut self) -> Namespace {
        match self.namespace {
            Some(_) => {
                let sql = format!("CREATE SCHEMA {}", ident_sql(ALT, self.dialect));
                self.exec(&sql).await;
                Namespace {
                    database: self.database.clone(),
                    schema: Some(ALT.to_string()),
                }
            }
            None => {
                let name = format!("{}_{ALT}", self.database);
                assert_scratch_name(&name);
                let sql = format!("CREATE DATABASE {}", ident_sql(&name, self.dialect));
                self.base
                    .fetch_query(None, &sql, 1, CancellationToken::new())
                    .await
                    .unwrap_or_else(|e| panic!("creating {name}: {e}"));
                self.extra_databases.push(name.clone());
                Namespace {
                    database: name,
                    schema: None,
                }
            }
        }
    }

    /// Run a statement inside `ns`, which may be [`Scratch::alt_namespace`]'s.
    pub async fn exec_in(&self, ns: &Namespace, sql: &str) -> ResultSet {
        self.db
            .clone()
            .with_database(Some(&ns.database))
            .fetch_query(Some(&ns.database), sql, ROW_CAP, CancellationToken::new())
            .await
            .unwrap_or_else(|e| {
                panic!(
                    "live tier statement failed: {e}
statement: {sql}"
                )
            })
    }

    /// `table`, quoted and qualified into `ns`.
    pub fn qualified_in(&self, ns: &Namespace, table: &str) -> String {
        let outer = ns.schema.as_deref().unwrap_or(&ns.database);
        format!(
            "{}.{}",
            ident_sql(outer, self.dialect),
            ident_sql(table, self.dialect)
        )
    }

    /// This scratch's own namespace, as the same pair.
    pub fn namespace_ref(&self) -> Namespace {
        Namespace {
            database: self.database.clone(),
            schema: self.namespace.map(str::to_string),
        }
    }

    /// `table`, quoted and qualified so a statement cannot land outside the
    /// scratch namespace even if the connection's scope were wrong.
    pub fn qualified(&self, table: &str) -> String {
        let outer = self.namespace.unwrap_or(&self.database);
        format!(
            "{}.{}",
            ident_sql(outer, self.dialect),
            ident_sql(table, self.dialect)
        )
    }

    /// Drop the scratch database now, and report failure as a failed test.
    ///
    /// The [`Drop`] guard covers a test that panicked before reaching this;
    /// calling it explicitly is what lets a test then assert the database is
    /// really gone.
    pub async fn teardown(mut self) {
        // Before the database this handle is attached to, so a failure to drop an
        // extra is still reported rather than orphaned by an early return.
        for name in std::mem::take(&mut self.extra_databases) {
            assert_scratch_name(&name);
            let sql = format!("DROP DATABASE {}", ident_sql(&name, self.dialect));
            self.base
                .fetch_query(None, &sql, 1, CancellationToken::new())
                .await
                .unwrap_or_else(|e| panic!("live tier could not drop {name}: {e}"));
        }
        let sql = self.drop_sql();
        self.base
            .fetch_query(None, &sql, 1, CancellationToken::new())
            .await
            .unwrap_or_else(|e| panic!("live tier could not drop {}: {e}", self.database));
        release_name(&self.database);
        // **Marked torn only once it is**, which is the whole point of the flag:
        // it is what tells `Drop` there is nothing left to do. Set at the top, a
        // panic from the extras loop above — the `unwrap_or_else` two lines up —
        // left `torn = true` with the *primary* database still there, and `Drop`
        // returned immediately. The tier's own rule is that it cleans up after
        // itself, and that is the one path where it silently did not.
        self.torn = true;
    }

    fn drop_sql(&self) -> String {
        assert_scratch_name(&self.database);
        let ident = ident_sql(&self.database, self.dialect);
        match self.dialect {
            // PostgreSQL refuses to drop a database anything is still connected
            // to, and this crate's one-connection-per-operation rule means the
            // last one closed a moment ago rather than definitely already.
            SqlDialect::Postgres => format!("DROP DATABASE {ident} WITH (FORCE)"),
            _ => format!("DROP DATABASE {ident}"),
        }
    }
}

impl Drop for Scratch {
    /// Tear down even when the test panicked — which is exactly when it matters,
    /// since a failing assertion skips every line after it.
    ///
    /// On its own thread with its own runtime, because a drop is synchronous and
    /// blocking on the current runtime from inside one deadlocks. **It does not
    /// panic while panicking**: a second panic during unwinding aborts the
    /// process and takes the real failure's message with it, so a teardown
    /// failure is printed in that case and raised only when the test was
    /// otherwise passing.
    fn drop(&mut self) {
        release_name(&self.database);
        if self.torn {
            return;
        }
        let base = self.base.clone();
        let name = self.database.clone();
        let mut plan: Vec<String> = self
            .extra_databases
            .iter()
            .inspect(|n| assert_scratch_name(n))
            .map(|n| format!("DROP DATABASE {}", ident_sql(n, self.dialect)))
            .collect();
        plan.push(self.drop_sql());
        let outcome = std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| e.to_string())?;
            // **Bounded.** A `DROP DATABASE` blocks behind an open transaction —
            // measured on MariaDB, and the manual-transaction tests are what
            // supply one — and an unbounded `block_on` inside a `Drop` hangs the
            // whole binary with no output at all. A timeout turns that into the
            // named leftover the message below is for.
            //
            // The `timeout` is constructed **inside** `block_on`: it builds a
            // `Sleep`, which panics with "there is no reactor running" if it is
            // made before the runtime is entered.
            rt.block_on(async {
                tokio::time::timeout(TEARDOWN_TIMEOUT, async {
                    for sql in &plan {
                        base.fetch_query(None, sql, 1, CancellationToken::new())
                            .await
                            .map_err(|e| e.to_string())?;
                    }
                    Ok(())
                })
                .await
                .unwrap_or_else(|_| Err(format!("teardown timed out after {TEARDOWN_TIMEOUT:?}")))
            })
        })
        .join();

        let failure = match outcome {
            Ok(Ok(())) => return,
            Ok(Err(e)) => e,
            Err(_) => "the teardown thread panicked".to_string(),
        };
        eprintln!("live tier left {name} behind: {failure}");
        assert!(
            std::thread::panicking(),
            "live tier could not drop {name}: {failure}"
        );
    }
}

/// Refuse any name this tier did not generate.
///
/// The check is on the *name*, not on the caller, because the caller is what
/// changes: the drop path is reached from a guard that runs during unwinding,
/// where nothing else is verifying anything.
pub fn assert_scratch_name(name: &str) {
    assert!(
        name.starts_with(PREFIX),
        "the live tier only ever touches {PREFIX}* databases; refusing {name:?}"
    );
    assert!(
        name.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'),
        "a scratch name is plain lower-case ASCII; refusing {name:?}"
    );
}
