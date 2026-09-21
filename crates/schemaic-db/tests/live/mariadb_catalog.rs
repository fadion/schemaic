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
//! **The two oracles below are MariaDB's, and for a long time this paragraph
//! said that was not a gap.** `SQL_FUNCTIONS` arrived in MariaDB 10.11 and MySQL
//! has no equivalent — `mysql.func` holds loadable UDFs, not builtins — so there
//! is no MySQL leg *of that kind* to write. That is why this module sits outside
//! `live_suite!`, for the same reason [`crate::pg_catalog`] does: the macro
//! expands one function into a test per leg.
//!
//! What it does not excuse is the consequence, which was stated here as a known
//! limitation and was in fact a live defect: `intel::FUNCTIONS` is a **two**-
//! engine catalog measured against **one** engine, so every name MySQL 8 does
//! not have was invisible and [`over_listing`] was green by construction over
//! forty-nine of them — each one offered to a MySQL 8 tab that cannot call it.
//!
//! [`each_server_is_only_credited_with_the_builtins_it_really_has`] is the
//! missing leg, and it is a different kind of oracle: **the parser**, which
//! answers the question the editor actually asks — can this server call this
//! name — and which both servers have. See its own doc.
//!
//! **Two oracles, not one and a hand-written excuse.** `SQL_FUNCTIONS` lists the
//! names the parser resolves through its function-creator hash, which leaves out
//! every builtin spelled as its own grammar rule — `LEFT`, `IF`, `AVG`,
//! `CURRENT_DATE` and the rest. Those are exactly MariaDB's *reserved words*, so
//! `information_schema.KEYWORDS` accounts for them without anybody writing them
//! down: the union of the two views is what the server claims, and
//! [`over_listing`] measures the catalog against that union.
//!
//! **The partition is computed in [`over_listing`], not stated here.** This
//! paragraph used to read "39 names covered by `KEYWORDS`, and the five left
//! over are [`MYSQL_ONLY`] to a name" — an arithmetic claim that does not add
//! up, since 39 + 5 is not the 49 it was partitioning, and the real figure is
//! 44. It was wrong when it was written and nothing computed it, which is the
//! whole argument for not writing a number down: `over_listing` asserts the
//! split adds up, so a release that moves it fails there instead of leaving a
//! sentence that reads plausibly and is false.

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
///
/// **`intel::MYSQL_ONLY` lower-cased, not a second list of the same names.**
/// The two exist for different jobs — that one decides what a MariaDB tab is
/// *offered*, this one excuses names from the server oracle below — and for a
/// while they were two literals, which is one edit away from an excuse that no
/// longer matches the filter it was written beside. Derived, so they cannot
/// disagree; `each_server_is_only_credited_with_the_builtins_it_really_has`
/// re-measures the shared list against both servers.
fn mysql_only() -> Vec<String> {
    schemaic_core::intel::MYSQL_ONLY
        .iter()
        .map(|n| n.to_ascii_lowercase())
        .collect()
}

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
    let functions = server_names(FUNCTION_ORACLE, 200).await;
    let keywords = server_names(KEYWORD_ORACLE, 500).await;
    let mut known = functions.clone();
    known.extend(keywords.iter().cloned());

    let ours = catalog_names();
    let unexplained: Vec<String> = ours
        .iter()
        .filter(|n| !known.contains(*n))
        .filter(|n| !mysql_only().contains(n))
        .cloned()
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

    // **The partition adds up, computed rather than written down.** The names
    // `SQL_FUNCTIONS` misses are exactly those with their own grammar rule,
    // and they are MariaDB's reserved words — so `KEYWORDS` has to account for
    // every one of them, and `MYSQL_ONLY` for the rest. The module doc used to
    // state this as "39 names covered by KEYWORDS, and the five left over",
    // which does not add up to the set it partitions and was never computed by
    // anything; the figure is 44.
    let by_keyword: Vec<&String> = ours
        .iter()
        .filter(|n| !functions.contains(*n) && keywords.contains(*n))
        .collect();
    let by_mysql_only: Vec<&String> = ours
        .iter()
        .filter(|n| !known.contains(*n) && mysql_only().contains(n))
        .collect();
    let not_in_functions = ours.iter().filter(|n| !functions.contains(*n)).count();
    assert_eq!(
        by_keyword.len() + by_mysql_only.len(),
        not_in_functions,
        "the two halves of the excuse do not add up to what `SQL_FUNCTIONS` \
         leaves out on {}: {} by KEYWORDS + {} by MYSQL_ONLY against {} \
         unreported",
        MARIADB.endpoint(),
        by_keyword.len(),
        by_mysql_only.len(),
        not_in_functions
    );
    assert!(
        !by_keyword.is_empty() && !by_mysql_only.is_empty(),
        "one half of the partition is empty on {}, so the sum above is not \
         measuring a partition at all",
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
    let all = mysql_only();
    let missing: Vec<&String> = all.iter().filter(|n| !ours.contains(*n)).collect();
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

/// **The MySQL leg this module said could not be written.**
///
/// `S6-L6-01`: the two oracles above measure a *two-engine* catalog against
/// *one* engine. Every name MariaDB does not report is excused by
/// [`MYSQL_ONLY`], and every name MySQL 8 does not have is invisible, because
/// nothing here ever asked MySQL anything. So `over_listing` was green **by
/// construction** over forty-nine names a MySQL 8 tab was being offered and
/// cannot call — `NVL`, `TO_CHAR`, the eight `COLUMN_*`, the nine `*_ORACLE`,
/// the three `WSREP_*` and the rest.
///
/// The module doc said "there is no MySQL leg to write" because MySQL ships no
/// `SQL_FUNCTIONS` view. That is true of a *catalogue* oracle and it is not the
/// only kind: **the parser is an oracle**, and it is the better one, because it
/// answers the question the editor actually asks — can this server call this
/// name. `SELECT <name>(1,2)` comes back `ERROR 1305` when the server has no
/// such function, and with some other error (wrong arity, wrong types, a syntax
/// error for a name with its own grammar rule) when it has one. Only 1305 is
/// read as absence.
///
/// **Both directions, from one pass.** What MySQL 8 lacks must be exactly
/// `intel`'s `MARIADB_ONLY`, and the same pass re-measures [`MYSQL_ONLY`] on
/// MariaDB — so the two lists that decide what each tab is offered are checked
/// against the two servers rather than against each other.
///
/// One pinned [`Session`] rather than 309 connections: this is the documented
/// exception to one-connection-per-operation, and 309 `Db::fetch_query` calls
/// would open 309 of them.
#[tokio::test(flavor = "multi_thread")]
async fn each_server_is_only_credited_with_the_builtins_it_really_has() {
    use schemaic_db::session::Session;

    for (target, expected) in [
        (&crate::endpoint::MYSQL, schemaic_core::intel::MARIADB_ONLY),
        (&MARIADB, schemaic_core::intel::MYSQL_ONLY),
    ] {
        if !target.enabled() {
            endpoint::note_skipped(target);
            continue;
        }
        // `mysql` as the scope: an unqualified unknown name is resolved as a
        // stored function in the current database, and with none selected the
        // server answers `1046 No database selected` instead of 1305 — which
        // would report every name as present and pass this test vacuously.
        let session = Session::open(&target.base_db(), Some("mysql"))
            .await
            .unwrap_or_else(|e| panic!("{}: could not pin a session: {e}", target.endpoint()));

        let mut absent: Vec<&str> = Vec::new();
        for f in FUNCTIONS {
            let sql = format!("SELECT {}(1,2)", f.name);
            if let Err(e) = session
                .fetch_query(&sql, 1, CancellationToken::new())
                .await
                .result
                && format!("{e}").contains("1305")
            {
                absent.push(f.name);
            }
        }
        session.close().await;

        let expected: HashSet<&str> = expected.iter().copied().collect();
        let absent: HashSet<&str> = absent.into_iter().collect();

        let unlisted: Vec<&&str> = absent.difference(&expected).collect();
        assert!(
            unlisted.is_empty(),
            "{} has no such function, and nothing in `intel` withholds it — so \
             a tab on this server is offered {} name(s) it cannot call: \
             {unlisted:?}",
            target.endpoint(),
            unlisted.len()
        );
        let stale: Vec<&&str> = expected.difference(&absent).collect();
        assert!(
            stale.is_empty(),
            "`intel` withholds {} name(s) from a tab on {} that this server \
             does have, so the popup is short by them: {stale:?}",
            stale.len(),
            target.endpoint()
        );
        // The pass really ran — an empty `absent` on both legs would satisfy
        // the first assertion having asked nothing.
        assert!(
            !absent.is_empty(),
            "no name at all came back absent on {}, so this oracle is not \
             answering",
            target.endpoint()
        );
    }
}
