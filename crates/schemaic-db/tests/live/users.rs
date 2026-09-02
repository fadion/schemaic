//! Accounts and privileges, against real servers.
//!
//! **The reads are of accounts that were already there; the writes make their
//! own.** An account is server-wide — it is not inside the scratch database and
//! would not go away with it — so the read half asks about the account the suite
//! connected as, which is the only one every leg is guaranteed to have. The
//! write half creates an account named with the tier's own
//! [`PREFIX`](crate::scratch::PREFIX) and drops it in a guard, which is the same
//! bargain [`Scratch`] makes with databases and is checked by the same name
//! rule: nothing here touches an account it did not create.
//!
//! What this covers is the part most likely to break, and the part nothing else
//! can: three servers, four different catalogues, a column set that differs
//! between MySQL 8 and MariaDB 10 in *both* directions, and six statements whose
//! spelling each engine has an opinion about. A query naming a column the server
//! hasn't got, or a `CREATE ROLE` clause it rejects, fails outright — and only a
//! live server says so.

use schemaic_core::ddl;
use schemaic_core::users::{
    self, AccountDraft, GrantDraft, GrantLevelKind, Principal, PrincipalKind,
};
use tokio_util::sync::CancellationToken;

use crate::endpoint::Target;
use crate::scratch::{PREFIX, Scratch, assert_scratch_name};

/// Find the account the suite is connected as, and fail with the whole list if
/// it isn't there — a list that came back but is missing the one account we know
/// exists means the query read something other than the accounts.
async fn connected_principal(target: &'static Target) -> Principal {
    let list = target
        .base_db()
        .fetch_principals()
        .await
        .unwrap_or_else(|e| panic!("fetch_principals on {}: {e}", target.endpoint()));
    let want = target.user();
    list.list
        .iter()
        .find(|p| p.name == want)
        .cloned()
        .unwrap_or_else(|| {
            let names: Vec<String> = list.list.iter().map(|p| p.display()).collect();
            panic!(
                "{} does not list the account it connected as ({want}); it listed: {names:?}",
                target.endpoint()
            )
        })
}

/// The list comes back, and it contains the account we are talking to it with.
pub async fn the_account_we_connected_as_is_in_the_list(target: &'static Target) {
    let me = connected_principal(target).await;
    // A MySQL account *is* the pair, and PostgreSQL has no host part at all —
    // the one place the two catalogues genuinely disagree about what an account
    // is, and the fold is what has to keep it straight.
    assert_eq!(
        me.host.is_some(),
        target.engine == schemaic_db::Engine::MySql,
        "{}: host part on {}",
        target.endpoint(),
        me.display()
    );
    assert!(!me.display().is_empty());
}

/// Its privileges come back as `GRANT` statements — the engine's own on MySQL,
/// reassembled from the catalogue on PostgreSQL, and the same shape either way.
pub async fn an_accounts_grants_come_back_as_grant_statements(target: &'static Target) {
    let me = connected_principal(target).await;
    let scratch = Scratch::create(target, "grants").await;
    let grants = scratch
        .db
        .fetch_grants(Some(&scratch.database), &me)
        .await
        .unwrap_or_else(|e| panic!("fetch_grants on {}: {e}", target.endpoint()));
    assert!(
        !grants.statements.is_empty(),
        "{}: no grants for {}, which cannot be true of the account we are connected as",
        target.endpoint(),
        me.display()
    );
    for s in &grants.statements {
        assert!(
            s.starts_with("GRANT "),
            "{}: {s:?} is not a GRANT statement",
            target.endpoint()
        );
    }
    scratch.teardown().await;
}

/// **Every grant list qualifies itself**, and one that covers a single database
/// names it.
///
/// Both halves are partial in a way the reader cannot see from the statements:
/// PostgreSQL's covers one database and no ownership or superuser rights;
/// MySQL's covers direct grants only. Neither may answer with `None`, which is
/// the claim "this is all of it".
pub async fn a_grant_list_says_which_database_it_covers_when_it_covers_only_one(
    target: &'static Target,
) {
    let me = connected_principal(target).await;
    let scratch = Scratch::create(target, "grantnote").await;
    let grants = scratch
        .db
        .fetch_grants(Some(&scratch.database), &me)
        .await
        .unwrap_or_else(|e| panic!("fetch_grants on {}: {e}", target.endpoint()));
    let note = grants.note.as_deref().unwrap_or_else(|| {
        panic!(
            "{}: a partial grant list qualified itself with nothing",
            target.endpoint()
        )
    });
    if target.grants_are_database_scoped {
        assert!(
            note.contains(&scratch.database),
            "{}: the note does not name the database it covers: {note}",
            target.endpoint()
        );
        // And what no ACL entry records — the omission that made
        // `pg_read_all_data` read as an account with no privileges at all.
        for claim in ["owning an object", "superuser"] {
            assert!(
                note.contains(claim),
                "{}: the note does not admit {claim:?}: {note}",
                target.endpoint()
            );
        }
    } else {
        assert!(
            note.contains("role"),
            "{}: the note does not name what it leaves out: {note}",
            target.endpoint()
        );
    }
    scratch.teardown().await;
}

/// A list fetched with **no database selected** does not quietly borrow another
/// database's privileges, and says so.
///
/// The bug this pins: with no database named, the PostgreSQL half connects to the
/// *maintenance* database, and it used to run the `pg_namespace`/`pg_class` ACL
/// queries there anyway — answering with that database's schema and table
/// privileges, for a database the user did not choose, under no note at all.
pub async fn a_grant_list_with_no_database_says_it_is_covering_none(target: &'static Target) {
    let me = connected_principal(target).await;
    let grants = target
        .base_db()
        .fetch_grants(None, &me)
        .await
        .unwrap_or_else(|e| panic!("fetch_grants on {}: {e}", target.endpoint()));
    if !target.grants_are_database_scoped {
        // A server-wide grant list needs no *database* qualification — but it is
        // not complete, and it used to say `note: None`, which claimed it was.
        // `SHOW GRANTS` is direct-only on both servers, so everything held
        // through a granted role is missing from it.
        let note = grants.note.as_deref().unwrap_or_else(|| {
            panic!(
                "{}: a direct-only grant list said nothing about the roles it does not expand",
                target.endpoint()
            )
        });
        assert!(
            note.contains("role"),
            "{}: the note does not name what it leaves out: {note}",
            target.endpoint()
        );
        return;
    }
    assert!(
        grants.note.is_some(),
        "{}: a database-scoped list with no database selected said nothing about its scope",
        target.endpoint()
    );
    for s in &grants.statements {
        for kw in [" ON TABLE ", " ON SCHEMA ", " ON SEQUENCE "] {
            assert!(
                !s.contains(kw),
                "{}: an object privilege was read from a database nobody selected: {s}",
                target.endpoint()
            );
        }
    }
}

/// **No password material reaches a caller.** MariaDB answers `SHOW GRANTS` with
/// the account's stored hash inline, and for `mysql_native_password` that hash is
/// the credential — this is the assertion that says the redaction is on the
/// fetch, not on one view that happens to call it.
pub async fn no_password_material_survives_the_fetch(target: &'static Target) {
    let me = connected_principal(target).await;
    let grants = target
        .base_db()
        .fetch_grants(None, &me)
        .await
        .unwrap_or_else(|e| panic!("fetch_grants on {}: {e}", target.endpoint()));
    for s in &grants.statements {
        assert!(
            !s.contains("'*"),
            "{}: a password hash reached the caller: {s}",
            target.endpoint()
        );
        if s.contains("IDENTIFIED") {
            assert!(
                s.contains("<hidden>"),
                "{}: an IDENTIFIED clause came through unredacted: {s}",
                target.endpoint()
            );
        }
    }
}

// ── the write half ───────────────────────────────────────────────────────────

/// An account this test made, dropped when it ends.
///
/// The account analogue of [`Scratch`], and it earns its own guard for the same
/// reason: an account outlives the test that made it, and one left behind on a
/// shared server is a login nobody remembers creating. Unlike a scratch database
/// it is **not** namespaced by anything the server enforces, so the name guard is
/// the whole safety story — [`assert_scratch_name`] is called on the way in and
/// again on the way out.
struct ScratchAccount {
    /// Scoped to the scratch database the plans run in.
    db: schemaic_db::Db,
    database: String,
    principal: Principal,
    dialect: schemaic_core::intel::SqlDialect,
    dropped: bool,
}

impl ScratchAccount {
    /// Create an account on `target`'s server through the **real emit-and-run
    /// path** — `ddl::account` → `ChangeSet::emit` → `Db::run_ddl` — because that
    /// path is what is under test, not the statement text on its own.
    async fn create(
        target: &'static Target,
        scratch: &Scratch,
        suffix: &str,
        kind: PrincipalKind,
    ) -> ScratchAccount {
        let name = format!("{PREFIX}{}_{}_{suffix}", std::process::id(), target.name);
        assert_scratch_name(&name);
        // 32 bytes on MySQL 8, 80 on MariaDB, 63 on PostgreSQL. Caught here
        // rather than as a truncated name two tests then share — and MySQL's
        // limit is the one that bites, since the prefix alone is twelve.
        assert!(
            name.len() <= 32,
            "account name {name:?} is {} bytes; shorten the suffix",
            name.len()
        );
        let dialect = scratch.dialect();
        let draft = AccountDraft {
            name: name.clone(),
            kind,
            // No password: it would be in the statement and in any failure
            // message, and nothing here logs in as this account.
            ..Default::default()
        };
        let principal = draft.principal(dialect);
        let me = ScratchAccount {
            db: scratch.db.clone(),
            database: scratch.database.clone(),
            principal,
            dialect,
            dropped: false,
        };
        me.run(ddl::Change::CreateAccount(Box::new(draft))).await;
        me
    }

    /// Put one account change through the whole path, and fail loudly if the
    /// server refuses it — which is the assertion, since a statement no engine
    /// accepts is exactly what this tier exists to catch.
    async fn run(&self, change: ddl::Change) {
        let summary = change.summary();
        let set = ddl::account(&self.principal.display(), self.dialect, change);
        let stmts = set.emit();
        assert!(
            !stmts.is_empty(),
            "{summary:?} emitted nothing on {:?}",
            self.dialect
        );
        self.db
            .run_ddl(&self.database, &stmts, CancellationToken::new())
            .await
            .unwrap_or_else(|e| {
                panic!("{summary:?} refused: {}\nstatements: {stmts:?}", e.message)
            });
    }

    /// What the server says this account may do, right now.
    async fn grants(&self) -> Vec<String> {
        self.db
            .fetch_grants(Some(&self.database), &self.principal)
            .await
            .unwrap_or_else(|e| panic!("fetch_grants: {e}"))
            .statements
    }

    async fn teardown(mut self) {
        let mine = self.principal.clone();
        self.drop_named(&mine).await;
    }

    /// Drop the account **as some other principal describes it** — the one the
    /// server listed rather than the one this test drafted. The two are not the
    /// same value, and the difference is exactly what a create-and-drop pair
    /// that never consults the catalogue cannot see.
    async fn drop_as(mut self, listed: &Principal) {
        assert_eq!(
            listed.name, self.principal.name,
            "drop_as is for the same account read back, not a different one"
        );
        let listed = listed.clone();
        self.drop_named(&listed).await;
    }

    async fn drop_named(&mut self, p: &Principal) {
        assert_scratch_name(&p.name);
        self.run(ddl::Change::DropAccount(Box::new(p.clone())))
            .await;
        self.dropped = true;
    }
}

impl Drop for ScratchAccount {
    /// Drop the account even when the test panicked — which is exactly when it
    /// matters, since a failing assertion skips every line after it.
    ///
    /// **It used to only print.** A panicking test therefore left a real login
    /// on a real server, named but present, and the tier's rule is that it
    /// cleans up after itself — its `Scratch` sibling does. On its own thread
    /// with its own runtime, because a drop is synchronous and blocking on the
    /// current runtime from inside one deadlocks; and it **does not panic while
    /// panicking**, so a teardown failure is printed rather than aborting the
    /// process and taking the real failure's message with it.
    fn drop(&mut self) {
        if self.dropped {
            return;
        }
        let db = self.db.clone();
        let database = self.database.clone();
        let who = self.principal.clone();
        let display = who.display();
        assert_scratch_name(&who.name);
        let stmts = ddl::account(
            &display,
            self.dialect,
            ddl::Change::DropAccount(Box::new(who)),
        )
        .emit();
        let outcome = std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| e.to_string())?;
            rt.block_on(async {
                db.run_ddl(&database, &stmts, CancellationToken::new())
                    .await
                    .map_err(|e| e.message)
            })
        })
        .join();

        let failure = match outcome {
            Ok(Ok(())) => return,
            Ok(Err(e)) => e,
            Err(_) => "the teardown thread panicked".to_string(),
        };
        eprintln!("live tier left account {display} behind: {failure}");
        assert!(
            std::thread::panicking(),
            "live tier could not drop {display}: {failure}"
        );
    }
}

/// A draft granting **one privilege, chosen from what this engine offers at the
/// database level** — which is the one level every engine here has, and the only
/// one a scratch database can be the subject of.
///
/// **The privilege is read off `privileges_for`, not named.** Naming one was the
/// first version and it was wrong on PostgreSQL: `SELECT` is spelled the same on
/// both engines and means the same thing on a table, but PostgreSQL's *database*
/// level carries only `CONNECT`, `CREATE` and `TEMPORARY` — a database is an
/// object there, not a shorthand for everything in it — and the server refused
/// the plan. Taking the engine's own first entry is what makes this one test
/// rather than three, and it exercises the list the form is built from.
fn one_privilege(scratch: &Scratch) -> (GrantDraft, String) {
    let order = users::privileges_for(scratch.dialect(), GrantLevelKind::Database);
    let privilege = order.first().expect("a database level with privileges");
    let mut d = GrantDraft {
        level: Some(GrantLevelKind::Database),
        qualifier: scratch.database.clone(),
        ..Default::default()
    };
    d.toggle(privilege, order);
    (d, privilege.to_string())
}

/// The whole create path, end to end: a plan this app emitted reaches the
/// server, and the server then lists the account it made.
pub async fn a_created_account_is_one_the_server_then_lists(target: &'static Target) {
    let scratch = Scratch::create(target, "mkuser").await;
    let account = ScratchAccount::create(target, &scratch, "u", PrincipalKind::User).await;

    let list = target
        .base_db()
        .fetch_principals()
        .await
        .unwrap_or_else(|e| panic!("fetch_principals: {e}"));
    assert!(
        list.list.iter().any(|p| p.name == account.principal.name),
        "{}: the account this test created is not in the list",
        target.endpoint()
    );

    account.teardown().await;
    scratch.teardown().await;
}

/// `CREATE ROLE` is a different statement with a different shape — no host, no
/// password — and each engine has its own opinion about it. Only a server says
/// whether ours is right.
///
/// **The role is read back out of the catalogue and every remaining statement
/// names *that* principal**, not the one this test built. That is the whole
/// point of the test and it is what the first version missed: `AccountDraft`
/// makes a role with no host, so a create-and-drop pair that never consults the
/// server round-trips a representation the server never produced. MariaDB
/// stores a role's host as `''`, and against that principal `SHOW GRANTS`
/// answered ERROR 1141 and `DROP ROLE` was a *syntax* error — three of the
/// browser's four role actions were broken on MariaDB while this test passed.
pub async fn a_created_role_is_one_the_server_accepts(target: &'static Target) {
    let scratch = Scratch::create(target, "mkrole").await;
    let role = ScratchAccount::create(target, &scratch, "r", PrincipalKind::Role).await;

    let listed = target
        .base_db()
        .fetch_principals()
        .await
        .unwrap_or_else(|e| panic!("fetch_principals: {e}"))
        .list
        .into_iter()
        .find(|p| p.name == role.principal.name)
        .unwrap_or_else(|| {
            panic!(
                "{}: the role this test created is not in the list",
                target.endpoint()
            )
        });

    // Reading its grants is what the browser does the moment the row is
    // clicked, and it is the statement MariaDB refused.
    scratch
        .db
        .fetch_grants(Some(&scratch.database), &listed)
        .await
        .unwrap_or_else(|e| {
            panic!(
                "{}: reading the grants of a role as the catalogue lists it: {e}",
                target.endpoint()
            )
        });

    // And the drop names the listed principal rather than the drafted one.
    role.drop_as(&listed).await;
    scratch.teardown().await;
}

/// The round trip the whole feature rests on: a privilege granted through the
/// form's own path comes back in the account's grant list, and a revoke takes it
/// off again.
///
/// **Read back through `fetch_grants`, not through a catalogue query of its
/// own** — that is the composition under test. The statement builders are
/// unit-tested; what nothing but this can check is that what the app *writes* and
/// what it *reads* name the same privilege.
pub async fn a_granted_privilege_comes_back_and_a_revoke_takes_it_off(target: &'static Target) {
    let scratch = Scratch::create(target, "grantrt").await;
    let account = ScratchAccount::create(target, &scratch, "g", PrincipalKind::User).await;

    let (draft, privilege) = one_privilege(&scratch);
    let change = ddl::grant_change(&draft, &account.principal).expect("a complete draft");
    account.run(change).await;

    let after_grant = account.grants().await;
    assert!(
        after_grant.iter().any(|s| s.contains(&privilege)),
        "{}: granted {privilege} is not in {after_grant:?}",
        target.endpoint()
    );

    let mut revoking = draft.clone();
    revoking.revoke = true;
    let change = ddl::grant_change(&revoking, &account.principal).expect("a complete draft");
    account.run(change).await;

    let after_revoke = account.grants().await;
    assert!(
        !after_revoke.iter().any(|s| s.contains(&privilege)),
        "{}: revoked {privilege} is still in {after_revoke:?}",
        target.endpoint()
    );

    account.teardown().await;
    scratch.teardown().await;
}

/// Role membership is its own statement shape on both engines, and its own pair
/// of variants — granted and taken back through the same path.
pub async fn a_granted_role_comes_back_and_a_revoke_takes_it_off(target: &'static Target) {
    let scratch = Scratch::create(target, "rolert").await;
    let role = ScratchAccount::create(target, &scratch, "sr", PrincipalKind::Role).await;
    let member = ScratchAccount::create(target, &scratch, "sm", PrincipalKind::User).await;

    let draft = GrantDraft {
        subject: users::GrantSubject::Role,
        role: role.principal.name.clone(),
        ..Default::default()
    };
    let change = ddl::grant_change(&draft, &member.principal).expect("a complete draft");
    member.run(change).await;

    let after = member.grants().await;
    assert!(
        after.iter().any(|s| s.contains(&role.principal.name)),
        "{}: the granted role is not in {after:?}",
        target.endpoint()
    );

    let mut revoking = draft.clone();
    revoking.revoke = true;
    let change = ddl::grant_change(&revoking, &member.principal).expect("a complete draft");
    member.run(change).await;

    // **The half the name promises and the test did not have.** `member.run`
    // only asserts the server *accepted* the statement, which a `REVOKE` naming
    // the wrong role does too — so this is what says the role actually came off,
    // and it mirrors the grant assertion above rather than being a new shape.
    let after = member.grants().await;
    assert!(
        !after.iter().any(|s| s.contains(&role.principal.name)),
        "{}: the revoked role is still in {after:?}",
        target.endpoint()
    );

    member.teardown().await;
    role.teardown().await;
    scratch.teardown().await;
}

/// And the drop really removes it, rather than reporting success against an
/// account that is still there.
pub async fn a_dropped_account_is_gone_from_the_list(target: &'static Target) {
    let scratch = Scratch::create(target, "rmuser").await;
    let account = ScratchAccount::create(target, &scratch, "d", PrincipalKind::User).await;
    let name = account.principal.name.clone();
    account.teardown().await;

    let list = target
        .base_db()
        .fetch_principals()
        .await
        .unwrap_or_else(|e| panic!("fetch_principals: {e}"));
    assert!(
        !list.list.iter().any(|p| p.name == name),
        "{}: {name} is still listed after being dropped",
        target.endpoint()
    );
    scratch.teardown().await;
}
