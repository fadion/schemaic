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
    /// How to switch an index **off** on this server, or `None` where nothing
    /// can. `{table}` and `{index}` are substituted.
    ///
    /// The two MySQL-family spellings differ and arrived in different releases
    /// (MySQL 8's `INVISIBLE`, MariaDB 10.6's `IGNORED`); PostgreSQL has no
    /// equivalent, so its leg returns early rather than asserting a property
    /// the engine does not have.
    pub disable_index_sql: Option<&'static str>,
    /// How this server declares a **functional** key part — an index over an
    /// expression rather than over a column — or `None` where it has none.
    /// `{table}` and `{index}` are substituted, and the expression is over the
    /// `a`/`b` integer columns the index tests seed.
    ///
    /// MySQL 8.0.13+ and PostgreSQL both take `((a + b))`; MariaDB 10.11
    /// rejects it outright (`ERROR 1064` at the inner paren). That asymmetry is
    /// not trivia: a functional key part is the one row where MySQL's
    /// `information_schema.STATISTICS` returns a **NULL** `COLUMN_NAME`, the
    /// bind was a non-`Option` `String`, and `from_row` panicked *inside the
    /// fetch task* — so one such index anywhere made the whole database
    /// unbrowsable, with the tree spinning for ever and no error at all. Two of
    /// the three legs could not reproduce it, which is exactly why the leg that
    /// can has to be named here rather than assumed.
    pub expression_index_sql: Option<&'static str>,
    /// Does a **DDL** plan roll back as a whole on this server?
    ///
    /// PostgreSQL's `run_ddl` wraps the plan in `BEGIN`/`ROLLBACK` and its DDL
    /// honours it, so a refused plan applies *nothing*. MySQL and MariaDB commit
    /// each `ALTER` as it runs, so the statements before the failure are on the
    /// table for good — which is why `DdlError::applied` exists and why the
    /// preview's failure message counts it.
    pub transactional_ddl: bool,
    /// Does **cancelling a statement inside a transaction abort the whole
    /// transaction** on this server?
    ///
    /// PostgreSQL treats a cancelled statement exactly as a failed one: the
    /// block enters the aborted state, everything after it answers `25P02`, and
    /// a `COMMIT` then behaves as a `ROLLBACK`. MySQL and MariaDB do not — a
    /// killed statement leaves the transaction open and committable, which is
    /// why `Session::fence_read` exists for the reads that must survive a
    /// dismissed panel there.
    ///
    /// Data on the target rather than an `if engine == Postgres` in a test body,
    /// for the reason at the top of [`crate::suite`] — and because it is the
    /// difference that decides what
    /// `a_stop_inside_a_manual_transaction_leaves_the_connection_usable` may
    /// assert about the commit that follows.
    pub cancel_aborts_transaction: bool,
    /// Does a grant list from this server cover **one database only**?
    ///
    /// PostgreSQL keeps schema, table and sequence privileges in the catalogue
    /// of the database holding the object, so one connection answers for one
    /// database and `Grants::note` says which. MySQL's grant tables are
    /// server-wide and `SHOW GRANTS` answers for every database at once, so
    /// there is nothing to qualify and the note is absent.
    ///
    /// Data on the target rather than an `if engine == Postgres` in a test body,
    /// for the reason at the top of [`crate::suite`].
    pub grants_are_database_scoped: bool,
    /// How this server adds non-key payload columns to a primary key, or
    /// `None` where it cannot.
    ///
    /// `PRIMARY KEY (id) INCLUDE (payload)` is PostgreSQL 11 and later; MySQL
    /// and MariaDB have no equivalent, so there is nothing there to mistake
    /// for a key column. The clause, not a bool, so the leg that has it also
    /// says how it spells it.
    pub primary_key_include: Option<&'static str>,
    /// Does a refused write on this server carry a **separate detail field**
    /// naming the offending value?
    ///
    /// PostgreSQL's `ErrorResponse` has `DETAIL` and `HINT` beside `message`,
    /// and for a constraint violation the value that broke it is only in
    /// `DETAIL` (`Key (pid)=(9) is not present in table "par".`) —
    /// `pg::db_err` dropped both, so the squiggle said a constraint was
    /// violated and never which value did it. MySQL and MariaDB put everything
    /// in one message and name the constraint rather than the value, so the
    /// value is not theirs to lose.
    ///
    /// Data on the target rather than an `if engine == Postgres` in a test
    /// body, for the reason at the top of [`crate::suite`].
    pub error_names_the_value: bool,
    /// The **view options** this server has, split where its grammar puts
    /// them: what goes between `CREATE ` and `VIEW `, and what goes after the
    /// body.
    ///
    /// Every view in the tier was a bare `CREATE VIEW … AS <body>`, so
    /// `TableInfo::view_options` was at its default in every fixture — and
    /// `diff_view` compares `draft.options != old_options` as one of the two
    /// triggers for a redefinition while `create_view_sql` restates them on the
    /// way out, neither half with a live assertion. A view created
    /// `WITH CHECK OPTION` that comes back without it stops refusing the writes
    /// it was created to refuse, and nothing would have said so.
    ///
    /// Two fields rather than one template because the two families put their
    /// clauses on opposite sides of the body: MySQL's `SQL SECURITY` is a
    /// prefix, the check option is a suffix on all three.
    pub view_prefix_options: &'static str,
    pub view_suffix_options: &'static str,
    /// How a trigger on this server says "uppercase the name being inserted".
    ///
    /// **The two engines model a trigger differently, not just spell it
    /// differently.** MySQL carries the body on the trigger itself; PostgreSQL
    /// has no body at all and calls a function with its own lifetime — dropping
    /// the trigger leaves it behind. So a leg supplies either a body or the SQL
    /// that creates a function plus the name to call, and never both.
    pub trigger_body: Option<&'static str>,
    pub trigger_function_ddl: Option<&'static str>,
    pub trigger_function_name: Option<&'static str>,
    /// A `WHEN` guard this server accepts on a trigger, and the columns an
    /// `UPDATE OF` may name — or `None`/empty where the engine has neither.
    ///
    /// `TriggerInfo::condition` and `TriggerInfo::update_columns` are the two
    /// fields the model widened itself for, and every fixture in the tier left
    /// them at their defaults: an emitter that dropped either on the
    /// drop-and-create every trigger edit performs would pass every test here,
    /// leaving a trigger that fires on every row and every column instead of
    /// the ones it was written for. `schema.rs` records that exact loss for
    /// SQLite, and PostgreSQL is the only leg in this tier that can catch it.
    pub trigger_condition: Option<&'static str>,
    pub trigger_update_columns: &'static [&'static str],
    /// How this server creates `purge_all()`: a function that deletes every row
    /// of `{table}` and returns `1`, so `SELECT purge_all()` is a write whose
    /// text is a read — what [`crate::enforced`] asks a read-only session to
    /// refuse. (Not `purge()`: `PURGE` is a reserved word on MySQL.)
    pub purging_function: &'static str,
    /// A statement that does nothing for a given number of seconds, in this
    /// server's spelling, with `{}` where the count goes and `{marker}` where
    /// the caller's marker goes — what the cancellation test interrupts. There
    /// is no portable way to ask a server to wait.
    ///
    /// **A template, not the finished statement.** It used to be
    /// `"SELECT SLEEP(5)"` in three places beside a `SLEEP_SECS = 5` the test
    /// compared against, documented as linked and held together by nothing:
    /// changing the constant left three servers sleeping for the old duration
    /// and the assertion measuring against the new one. [`Target::sleep_sql`]
    /// is the one place the two meet.
    ///
    /// **The marker is the caller's, not a constant**, because the two
    /// cancellation tests of a leg run concurrently under libtest and
    /// [`Target::running_sleeps`] reads a *server-wide* view. With one shared
    /// marker each test's probe counted the other test's statement, and a
    /// failure there reported "the server is still running the statement" about
    /// a statement belonging to a different test. `74387d2` recognised this for
    /// the script test's *arming* probe and routed around it; the two `still ==
    /// 0` assertions kept the coupling.
    sleep_template: &'static str,
    /// How to ask this server whether a [`Target::sleep_sql`] carrying a given
    /// marker is **still running** — one row, one column, the count. `{head}`
    /// and `{tail}` are the marker's two halves; see
    /// [`Target::running_sleeps`].
    ///
    /// The other half of the cancellation test, and the half it did not have: it
    /// measured how long the *client* took to return `Cancelled`, which
    /// `tokio::select!` answers on its own. Deleting `kill_query` and
    /// `cancel_query` from both arms left all six legs passing in ~250 ms while
    /// three servers slept on.
    ///
    /// The marker is split in the middle of this pattern so the probe does not
    /// count *itself*: its own text is in the same view it reads.
    running_sleeps_sql: &'static str,
    /// The types this server is asked to round-trip, and the ones only it has.
    /// Two slices rather than one so MySQL and MariaDB can share the twenty they
    /// agree on and still each own the one they do not — see [`cases`].
    types: &'static [TypeCase],
    extra_types: &'static [TypeCase],
    /// How many type cases this leg **has**, written out.
    ///
    /// **A hand-maintained number, deliberately, and it is the whole guard.**
    /// `report`'s "it ran them all" assertion took `expected` as
    /// `type_cases().count()` — the same iterator `ran` counted while walking —
    /// so both sides were the same pure function of the same data and
    /// `ran == expected` was a tautology. Delete PostgreSQL's whole numeric
    /// family and `ran` falls from 25 to 20, `expected` falls from 25 to 20 in
    /// the same step, and both matrix tests report green having asserted nothing
    /// about the five types that vanished — the *exact* scenario `report`'s own
    /// doc says it exists to catch. (The `TYPE_CASE_FLOOR = 20` it replaced
    /// would have caught that one; the equality that replaced the floor caught
    /// nothing, and was documented as the stronger check.)
    ///
    /// So it has to be a value the leg's own data cannot move. Adding a case
    /// means editing this number too, and
    /// `every_leg_declares_the_number_of_cases_it_has` is what says so.
    expected_cases: usize,
    /// The same, for the write-back matrix, which runs only the cases marked
    /// [`TypeCase::writable`] — a raw-bytes cell shows a placeholder and refuses
    /// to be edited, so only its rendering is asserted.
    ///
    /// A second number rather than `expected_cases` minus a computed count, for
    /// the reason the first one exists: anything derived from the slices moves
    /// with them.
    expected_writable_cases: usize,
    /// How many of this leg's writable cases can be a **primary key** — and so
    /// how many of them reach `row_key` on a value the server rendered, rather
    /// than on an `INTEGER` the test wrote down itself.
    ///
    /// A hand-written number for the reason [`Target::expected_cases`] is one,
    /// and this is the second instance of the same failure: the guard was
    /// `assert!(keyed > 0)` — a floor of **one** over eighteen keyable types.
    /// A change to `edit.rs`'s read-only key refusal that widened it from
    /// float/binary to every non-integer type would send every case down the
    /// fallback path and leave the test green, `tinyint_min` alone keeping the
    /// count above zero; `row_key` would then be exercised on nothing but
    /// integers. The project has already paid for a floor like this once, at
    /// `suite.rs`'s `TYPE_CASE_FLOOR`.
    expected_keyed_cases: usize,
}

/// [`Target::purging_function`] for both MySQL-family legs. `DETERMINISTIC` is
/// what lets a server with binary logging on accept it from a user without
/// `log_bin_trust_function_creators`; it is a lie about the function, and the
/// function exists only inside a scratch database.
const MYSQL_PURGING_FUNCTION: &str = "CREATE FUNCTION purge_all() RETURNS INT DETERMINISTIC \
     MODIFIES SQL DATA BEGIN DELETE FROM {table}; RETURN 1; END";

pub static MARIADB: Target = Target {
    name: "mariadb",
    engine: Engine::MySql,
    env: "SCHEMAIC_IT_MARIADB",
    default_port: 3306,
    default_user: "schemaic",
    namespace: None,
    binary_type: "VARBINARY(4)",
    non_transactional: Some("ENGINE=MyISAM"),
    disable_index_sql: Some("ALTER TABLE {table} ALTER INDEX {index} IGNORED"),
    expression_index_sql: None,
    transactional_ddl: false,
    cancel_aborts_transaction: false,
    grants_are_database_scoped: false,
    primary_key_include: None,
    error_names_the_value: false,
    view_prefix_options: "SQL SECURITY INVOKER ",
    view_suffix_options: " WITH CASCADED CHECK OPTION",
    trigger_body: Some("SET NEW.name = UPPER(NEW.name)"),
    trigger_function_ddl: None,
    trigger_function_name: None,
    trigger_condition: None,
    trigger_update_columns: &[],
    purging_function: MYSQL_PURGING_FUNCTION,
    sleep_template: "SELECT SLEEP({}) /* {marker} */",
    running_sleeps_sql: "SELECT COUNT(*) FROM information_schema.PROCESSLIST \n         WHERE INFO LIKE CONCAT('%{head}', '{tail}%')",
    types: cases::MYSQL_FAMILY,
    extra_types: cases::MARIADB_ONLY,
    expected_cases: 27,
    expected_writable_cases: 26,
    expected_keyed_cases: 20,
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
    disable_index_sql: Some("ALTER TABLE {table} ALTER INDEX {index} INVISIBLE"),
    expression_index_sql: Some("CREATE INDEX {index} ON {table} ((a + b))"),
    transactional_ddl: false,
    cancel_aborts_transaction: false,
    grants_are_database_scoped: false,
    primary_key_include: None,
    error_names_the_value: false,
    view_prefix_options: "SQL SECURITY INVOKER ",
    view_suffix_options: " WITH CASCADED CHECK OPTION",
    trigger_body: Some("SET NEW.name = UPPER(NEW.name)"),
    trigger_function_ddl: None,
    trigger_function_name: None,
    trigger_condition: None,
    trigger_update_columns: &[],
    purging_function: MYSQL_PURGING_FUNCTION,
    sleep_template: "SELECT SLEEP({}) /* {marker} */",
    running_sleeps_sql: "SELECT COUNT(*) FROM information_schema.PROCESSLIST \n         WHERE INFO LIKE CONCAT('%{head}', '{tail}%')",
    types: cases::MYSQL_FAMILY,
    extra_types: cases::MYSQL_ONLY,
    expected_cases: 27,
    expected_writable_cases: 26,
    expected_keyed_cases: 20,
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
    disable_index_sql: None,
    expression_index_sql: Some("CREATE INDEX {index} ON {table} ((a + b))"),
    transactional_ddl: true,
    cancel_aborts_transaction: true,
    grants_are_database_scoped: true,
    primary_key_include: Some(" INCLUDE (payload)"),
    error_names_the_value: true,
    view_prefix_options: "",
    view_suffix_options: " WITH CASCADED CHECK OPTION",
    trigger_body: None,
    trigger_function_ddl: Some(
        "CREATE FUNCTION upper_name() RETURNS trigger AS $$          BEGIN NEW.name := UPPER(NEW.name); RETURN NEW; END $$ LANGUAGE plpgsql",
    ),
    trigger_function_name: Some("upper_name"),
    trigger_condition: Some("NEW.name IS NOT NULL"),
    trigger_update_columns: &["name"],
    purging_function: "CREATE FUNCTION purge_all() RETURNS int LANGUAGE sql AS \
         $$ DELETE FROM {table}; SELECT 1 $$",
    sleep_template: "SELECT pg_sleep({}) /* {marker} */",
    running_sleeps_sql: "SELECT count(*) FROM pg_stat_activity \n         WHERE state = 'active' AND query LIKE '%{head}' || '{tail}%'",
    types: cases::POSTGRES,
    extra_types: &[],
    expected_cases: 28,
    expected_writable_cases: 27,
    expected_keyed_cases: 24,
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

    /// A handle on this server as **some other account**, attached to no
    /// database.
    ///
    /// The one assertion that can tell a created-with-the-right-password
    /// account from a created-with-a-different-one: both statements are
    /// accepted by the server, so only a login distinguishes them. See
    /// `users::a_created_account_can_log_in_with_the_password_it_was_given`.
    pub fn db_as(&self, user: &str, password: &str) -> Db {
        Db::from_parts(
            self.engine,
            self.var("HOST", "127.0.0.1"),
            self.port(),
            user.to_string(),
            password.to_string(),
            String::new(),
        )
    }

    /// Is an account on this server a **name and a host**, rather than a name?
    ///
    /// The one place the two catalogues genuinely disagree about what an account
    /// *is*: `DROP USER 'n'@'h'` names both halves on the MySQL family, and
    /// PostgreSQL has no host part at all. One definition, because the host
    /// assertions and the host fixtures have to agree about which legs have one.
    pub fn accounts_have_hosts(&self) -> bool {
        self.engine == Engine::MySql
    }

    /// The account the suite connects as — which is also the one account every
    /// leg is guaranteed to have, and so the one a read-only test can name.
    pub fn user(&self) -> String {
        self.var("USER", self.default_user)
    }

    /// Every type case this server answers for.
    pub fn type_cases(&self) -> impl Iterator<Item = &'static TypeCase> {
        self.types.iter().chain(self.extra_types)
    }

    /// How many cases the matrix must run for this leg — see
    /// [`Target::expected_cases`], and do **not** replace this with
    /// `type_cases().count()`: that is the tautology it exists to end.
    pub fn expected_cases(&self) -> usize {
        self.expected_cases
    }

    /// [`Target::expected_cases`] for the write-back matrix.
    pub fn expected_writable_cases(&self) -> usize {
        self.expected_writable_cases
    }

    /// [`Target::expected_keyed_cases`] — how many writable cases this leg can
    /// key on their own type.
    pub fn expected_keyed_cases(&self) -> usize {
        self.expected_keyed_cases
    }

    /// The statement that makes this server wait for
    /// [`crate::runtime::SLEEP_SECS`] seconds, tagged with `marker` so
    /// [`Target::running_sleeps`] can count this caller's sleeps and nobody
    /// else's.
    ///
    /// The one place the template and the constant meet — see
    /// [`Target::sleep_template`].
    pub fn sleep_sql(&self, marker: &str) -> String {
        self.sleep_template
            .replace("{}", &crate::runtime::SLEEP_SECS.to_string())
            .replace("{marker}", marker)
    }

    /// How many [`Target::sleep_sql`] statements carrying `marker` this server
    /// is running right now — the question the cancellation test asks the
    /// *server*.
    ///
    /// **The marker goes into the pattern in two halves**, so the probe's own
    /// text — which sits in the very view it reads — does not match. Splitting
    /// here rather than at the call site keeps that property a fact about this
    /// function instead of a rule every caller has to remember; `debug_assert`
    /// is the floor, because a marker short enough to split badly would make
    /// the probe count itself and every assertion downstream would be about the
    /// wrong statement.
    pub async fn running_sleeps(&self, db: &Db, marker: &str) -> u64 {
        debug_assert!(
            marker.len() >= 4 && marker.is_ascii(),
            "a cancel marker must be at least four ASCII bytes so it can be split"
        );
        let (head, tail) = marker.split_at(marker.len() / 2);
        let sql = self
            .running_sleeps_sql
            .replace("{head}", head)
            .replace("{tail}", tail);
        let rs = db
            .fetch_query(None, &sql, 10, Default::default())
            .await
            .unwrap_or_else(|e| panic!("{}: could not read the sleep probe: {e}", self.endpoint()));
        rs.cell(0, 0)
            .and_then(|c| c.text().parse().ok())
            .unwrap_or_else(|| panic!("{}: the sleep probe returned no count", self.endpoint()))
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

/// **Every leg's declared case counts match its slices — and needs no server.**
///
/// The other half of [`Target::expected_cases`]. The number has to be
/// hand-maintained or the "it ran them all" assertion is a tautology; this is
/// what makes forgetting to maintain it loud, and it is the only test in this
/// tier that asserts something without connecting to anything. So adding a case
/// fails *here*, with the right number in the message, rather than passing
/// silently in the matrix.
///
/// **Where "here" is, exactly.** Needing no server and being *built* are
/// different properties, and this test has only the first: the target declares
/// `required-features = ["live-tests"]`, so a developer's `cargo test
/// --workspace` does not compile it and cannot run this. What does run it is
/// CI — `.github/workflows/ci.yml`'s `live` job builds this target and runs it
/// against `mariadb:11`, `mysql:8.4` and `postgres:16` on every push and every
/// pull request, blocking, with no `continue-on-error` — so a stale constant is
/// a red required check on the next push rather than a number nobody notices.
/// That gap between the local bar and the CI bar is the deliberate purity
/// boundary `Cargo.toml`'s feature block describes, not a hole; this paragraph
/// is here because the sentence above it was once read as promising the local
/// run too.
#[test]
fn every_leg_declares_the_number_of_cases_it_has() {
    for t in ALL {
        assert_eq!(
            t.type_cases().count(),
            t.expected_cases,
            "{}: expected_cases says {} and the slices hold {} — update the \
             constant in endpoint.rs, which is what stops the matrix asserting \
             nothing",
            t.name,
            t.expected_cases,
            t.type_cases().count(),
        );
        assert_eq!(
            t.type_cases().filter(|c| c.writable).count(),
            t.expected_writable_cases,
            "{}: expected_writable_cases says {} and the slices hold {}",
            t.name,
            t.expected_writable_cases,
            t.type_cases().filter(|c| c.writable).count(),
        );
        // **The third number, which this gate was not asserting.**
        // `expected_keyed_cases`' own doc says it exists "for the reason
        // `expected_cases` is one" — and then the guard that makes forgetting to
        // maintain such a number loud checked the other two and not it. There is
        // no slice to compare it against without a server (whether a case keys
        // on its own type is the *server's* answer, which is why the matrix
        // asserts it), so the bounds are what can be checked here: a keyable
        // case is a writable one, and a leg that declares none has stopped
        // testing the thing this number exists for.
        assert!(
            t.expected_keyed_cases <= t.expected_writable_cases,
            "{}: expected_keyed_cases says {} of {} writable cases key on their \
             own type, which is more than there are",
            t.name,
            t.expected_keyed_cases,
            t.expected_writable_cases,
        );
        assert!(
            t.expected_keyed_cases > 0,
            "{}: expected_keyed_cases is zero, so the matrix's `row_key` \
             assertion has nothing left to be about",
            t.name,
        );
    }
}

/// Say, **where a developer will see it**, that a leg was excluded.
///
/// The tier's design is explicit that a silent green is the thing it exists to
/// prevent, and it names one deliberate exception: a leg left out of
/// `SCHEMAIC_IT_ENGINES` returns without asserting. The whole mitigation for
/// that exception is this notice, and [`engines_var`]'s own doc states the
/// bargain it rests on — an exclusion is tolerable when a developer typed it on
/// their own machine and *can see it on stderr*.
///
/// **They could not.** It was `eprintln!` inside a `#[tokio::test]` body, and
/// libtest captures a test's `print!`/`eprint!` and prints it **only for
/// failing tests** — which a skipped leg is not. So narrowing the tier to
/// `mariadb` reported green across all three legs' names with nothing on screen
/// separating the ones that ran from the ~two-thirds that returned
/// immediately. That matters beyond tidiness: this tier is the only call-site
/// coverage the write-back 1-row net has on MySQL and PostgreSQL, so a
/// developer who narrowed the tier, fixed a MariaDB failure and re-ran to green
/// has been told nothing about the other two.
///
/// `writeln!` on the locked handle writes past libtest's capture-aware macro
/// path, and the once-per-leg guard keeps ninety-odd identical lines down to
/// one. `no_skip_notice_goes_through_the_captured_macro` is what holds the
/// spelling, because a `#[test]` cannot observe libtest's capture about its own
/// run — the same argument the crate's source gates all make.
pub fn note_skipped(target: &'static Target) {
    use std::io::Write as _;
    use std::sync::OnceLock;
    static SAID: OnceLock<std::sync::Mutex<std::collections::HashSet<&'static str>>> =
        OnceLock::new();
    let said = SAID.get_or_init(Default::default);
    let first = said
        .lock()
        .map(|mut s| s.insert(target.name))
        .unwrap_or(false);
    if !first {
        return;
    }
    let mut err = std::io::stderr().lock();
    let _ = writeln!(
        err,
        "live: {} is not in SCHEMAIC_IT_ENGINES — its tests asserted nothing",
        target.name
    );
    let _ = err.flush();
}

/// [`note_skipped`]'s sibling, for a leg that **runs** and has nothing to assert
/// on this engine.
///
/// The other silent green the tier admits, and the more common one: a test whose
/// subject is one engine's feature returns early on the other two. Two such
/// notices in `routines.rs` went through `eprintln!`, which libtest prints only
/// for a *failing* test — so four of six leg-tests reported green with nothing
/// on screen, on every run including CI's, under a doc claiming the opposite.
///
/// Not deduped the way `note_skipped` is: that one is about a target and would
/// otherwise repeat once per test, while this is about a *leg* — a different
/// reason each time, and repeating it is the point.
pub fn note_no_op(target: &'static Target, reason: &str) {
    use std::io::Write as _;
    let mut err = std::io::stderr().lock();
    let _ = writeln!(
        err,
        "live: {} {reason} — this test asserted nothing",
        target.name
    );
    let _ = err.flush();
}
