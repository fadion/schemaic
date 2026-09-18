//! `intel::FUNCTIONS` against the MariaDB the tier is pointed at.
//!
//! The second instance of [`crate::pg_catalog`]'s rule, for the other
//! hand-written catalog. `FUNCTIONS` decides whether the editor squiggles a
//! name as a misspelling, so a function MariaDB really has and the list really
//! lacks is a warning under correct SQL — and, unlike PostgreSQL's, this list
//! was written from memory of the documentation with nothing to check it
//! against. `information_schema.SQL_FUNCTIONS` is the server's own answer, so
//! it is the thing to ask.
//!
//! **MariaDB-only, and that is not a gap in the tier.** `SQL_FUNCTIONS` arrived
//! in MariaDB 10.11 and MySQL has no equivalent — `mysql.func` holds loadable
//! UDFs, not builtins — so there is no MySQL leg to write. That is also why this
//! module sits outside `live_suite!`, for the same reason [`crate::pg_catalog`]
//! does: the macro expands one function into a test per leg, and two of the
//! three legs cannot answer the question. The consequence is worth stating
//! plainly: a builtin **MySQL 8 added and MariaDB never got** is unguarded here,
//! and [`MYSQL_ONLY`] is the list of the ones already known.
//!
//! **Two oracles, not one and a hand-written excuse.** `SQL_FUNCTIONS` lists the
//! names the parser resolves through its function-creator hash, which leaves out
//! every builtin spelled as its own grammar rule — `LEFT`, `IF`, `AVG`,
//! `CURRENT_DATE` and the rest. Those are exactly MariaDB's *reserved words*, so
//! `information_schema.KEYWORDS` accounts for them without anybody writing them
//! down: the union of the two views is what the server claims, and
//! [`over_listing`] measures the catalog against that union. The partition is
//! exact on 10.11.14 — 39 names covered by `KEYWORDS`, and the five left over
//! are [`MYSQL_ONLY`] to a name.

use std::collections::HashSet;

use schemaic_core::intel::FUNCTIONS;
use tokio_util::sync::CancellationToken;

use crate::endpoint::{self, MARIADB};

/// Every builtin the parser registers by name.
const FUNCTION_ORACLE: &str = "SELECT `FUNCTION` FROM information_schema.SQL_FUNCTIONS";

/// The reserved words, which is where the builtins with their own grammar rule
/// are to be found — see the module docs.
const KEYWORD_ORACLE: &str = "SELECT `WORD` FROM information_schema.KEYWORDS";

/// Names `SQL_FUNCTIONS` reports that the parser will not accept as a call, so
/// offering them would complete to a syntax error and squiggling them is
/// impossible anyway.
///
/// `SCHEMAS` is the only one on 10.11.14, and it is a *reserved word* — `SHOW
/// SCHEMAS` — which is what stops `SELECT SCHEMAS()` from parsing.
/// Reserved-word-ness alone does not explain it, so this cannot be read off
/// `KEYWORDS`: `PASSWORD`, `OLD_PASSWORD` and `COLUMN_CHECK` are reserved too
/// and all three are callable. [`the_uncallable_name_is_still_uncallable`] is
/// what keeps the exception from outliving the wart.
const NOT_CALLABLE: &[&str] = &["schemas"];

/// Builtins MySQL 8 has and MariaDB 10.11 does not, so neither server view
/// reports them and [`over_listing`] must account for them by hand.
///
/// `FUNCTIONS` is one catalog for two engines, so carrying them is correct: the
/// alternative is squiggling `UUID_TO_BIN` for the MySQL user who typed it.
/// Verified absent by calling each on 10.11.14 and getting
/// `FUNCTION … does not exist` rather than a wrong-argument error.
const MYSQL_ONLY: &[&str] = &[
    "bin_to_uuid",
    "is_uuid",
    "json_storage_size",
    "regexp_like",
    "uuid_to_bin",
];

/// The server knows a name this catalog does not — a false positive waiting for
/// whoever types it.
#[tokio::test(flavor = "multi_thread")]
async fn every_function_the_server_reports_is_in_the_catalog() {
    if !MARIADB.enabled() {
        endpoint::note_skipped(&MARIADB);
        return;
    }
    let server = server_names(FUNCTION_ORACLE, 200).await;
    let ours = catalog_names();

    let missing: Vec<&str> = server
        .iter()
        .map(String::as_str)
        .filter(|n| !ours.contains(*n))
        .filter(|n| !NOT_CALLABLE.contains(n))
        .collect();

    assert!(
        missing.is_empty(),
        "{} reports {} function(s) `intel::FUNCTIONS` does not carry, so each \
         would be squiggled as a misspelling of whatever it happens to \
         resemble: {missing:?}",
        MARIADB.endpoint(),
        missing.len()
    );
}

/// The catalog carries a name neither server view reports, and this test says
/// which kinds are allowed to be there.
///
/// A name that is neither the server's nor in [`MYSQL_ONLY`] is invented, and an
/// invented name silently suppresses a genuine typo warning — the failure the
/// checker cannot see from the inside.
#[tokio::test(flavor = "multi_thread")]
async fn over_listing() {
    if !MARIADB.enabled() {
        endpoint::note_skipped(&MARIADB);
        return;
    }
    let mut known = server_names(FUNCTION_ORACLE, 200).await;
    known.extend(server_names(KEYWORD_ORACLE, 500).await);

    let unexplained: Vec<String> = catalog_names()
        .into_iter()
        .filter(|n| !known.contains(n))
        .filter(|n| !MYSQL_ONLY.contains(&n.as_str()))
        .collect();

    assert!(
        unexplained.is_empty(),
        "`intel::FUNCTIONS` carries {} name(s) neither \
         `information_schema.SQL_FUNCTIONS` nor `information_schema.KEYWORDS` \
         reports on {}. Either they are MySQL 8's and belong in MYSQL_ONLY, or \
         they are invented and would suppress a genuine typo warning: \
         {unexplained:?}",
        unexplained.len(),
        MARIADB.endpoint()
    );
}

/// Every name in [`MYSQL_ONLY`] really is in the catalog, so the allowance
/// [`over_listing`] grants cannot outlive what it was granted for.
///
/// Without this, deleting `UUID_TO_BIN` from `FUNCTIONS` leaves both tests above
/// green while the MySQL user who types it gets a squiggle.
#[tokio::test(flavor = "multi_thread")]
async fn the_mysql_only_names_are_in_the_catalog() {
    if !MARIADB.enabled() {
        endpoint::note_skipped(&MARIADB);
        return;
    }
    let ours = catalog_names();
    let missing: Vec<&&str> = MYSQL_ONLY.iter().filter(|n| !ours.contains(**n)).collect();
    assert!(
        missing.is_empty(),
        "`over_listing` excuses these from the server's answer, and \
         `intel::FUNCTIONS` does not carry them either, so nothing holds them \
         at all: {missing:?}"
    );
}

/// Every name [`NOT_CALLABLE`] excuses really is rejected by this server, and
/// really is absent from the catalog.
///
/// The exception is a claim about MariaDB's parser, so the parser is what
/// answers it — not a comment. If a release makes `SCHEMAS()` parse, the first
/// half fails and the name is due in `FUNCTIONS`; if somebody adds it to
/// `FUNCTIONS` anyway, the second half fails and the completion popup does not
/// get to offer a name that cannot be typed.
#[tokio::test(flavor = "multi_thread")]
async fn the_uncallable_name_is_still_uncallable() {
    if !MARIADB.enabled() {
        endpoint::note_skipped(&MARIADB);
        return;
    }
    let ours = catalog_names();
    for name in NOT_CALLABLE {
        let sql = format!("SELECT {name}()");
        let err = MARIADB
            .base_db()
            .fetch_query(None, &sql, 1, CancellationToken::new())
            .await
            .err()
            .unwrap_or_else(|| {
                panic!(
                    "`{name}` is excused from \
                     `every_function_the_server_reports_is_in_the_catalog` as \
                     unparseable, but {} accepted `{sql}`, so it belongs in \
                     `intel::FUNCTIONS` like any other builtin",
                    MARIADB.endpoint()
                )
            });
        assert!(
            format!("{err}").contains("syntax"),
            "`{name}` is excused as unparseable and {} rejected `{sql}` for \
             some other reason, so the excuse is no longer the true one: {err}",
            MARIADB.endpoint()
        );
        assert!(
            !ours.contains(*name),
            "`intel::FUNCTIONS` carries `{name}`, which {} will not parse as a \
             call — completing it would produce a syntax error",
            MARIADB.endpoint()
        );
    }
}

/// The rows one of the two oracles returns, lower-cased.
///
/// `floor` is the count below which the view is not answering rather than
/// answering short: an empty result would pass
/// [`every_function_the_server_reports_is_in_the_catalog`] having asserted
/// nothing, which is the decoration this tier exists to avoid. 10.11.14 reports
/// 261 functions and 696 keywords.
async fn server_names(sql: &str, floor: usize) -> HashSet<String> {
    let rs = MARIADB
        .base_db()
        .fetch_query(None, sql, 10_000, CancellationToken::new())
        .await
        .unwrap_or_else(|e| {
            panic!(
                "live tier could not read information_schema on {}: {e}\nstatement: {sql}",
                MARIADB.endpoint()
            )
        });
    let names: HashSet<String> = (0..rs.row_count())
        .filter_map(|r| rs.cell(r, 0).map(|c| c.text().to_ascii_lowercase()))
        .collect();
    assert!(
        names.len() >= floor,
        "only {} name(s) came back from `{sql}`, so this oracle is not \
         answering and the tests above would pass having asserted nothing",
        names.len()
    );
    names
}

fn catalog_names() -> HashSet<String> {
    FUNCTIONS
        .iter()
        .map(|f| f.name.to_ascii_lowercase())
        .collect()
}
