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
//! **Three oracles, not one and a hand-written excuse.** `SQL_FUNCTIONS` lists
//! the names the parser resolves through its function-creator hash, which leaves
//! out every builtin spelled as its own grammar rule — `LEFT`, `IF`, `AVG`,
//! `CURRENT_DATE` and the rest. Those are exactly MariaDB's *reserved words*, so
//! `information_schema.KEYWORDS` accounts for them without anybody writing them
//! down.
//!
//! **And that was said here to be the whole of it, which was false.** This
//! paragraph read "the union of the two views is what the server claims"; it is
//! not. A builtin registered natively and resolved by its own path is in
//! *neither* view — `ST_Area` is absent from `SQL_FUNCTIONS` while the same
//! server answers `SELECT st_area()` with "incorrect parameter count in the call
//! to **native function** 'st_area'", the server's own words for a builtin. The
//! entire OGC spatial family sat in that gap, and `ST_Length` was squiggling
//! under correct SQL on the default engine while every test in this file was
//! green. So the third oracle is the **parser**, which both servers have and
//! which answers the question the editor actually asks; [`over_listing`]
//! measures the catalog against all three, and
//! [`no_name_this_server_can_call_is_squiggled_under_correct_sql`] measures the
//! *consequence* rather than the membership.
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

/// The builtins the parser registers **by name**, in its function-creator hash.
///
/// Not "every builtin", which is what this doc used to say and what the module
/// doc was built on: a natively-registered builtin resolved by its own path is
/// not in here. See the module doc, and
/// [`no_name_this_server_can_call_is_squiggled_under_correct_sql`].
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

/// Builtins **MariaDB gained after 10.11**, which the catalog carries and an
/// older server does not report.
///
/// **The catalog spans versions as well as engines, and nothing else here said
/// so.** `MARIADB_ONLY` and `MYSQL_ONLY` answer "which *engine* has this name";
/// there was no term for "which *version*", so the two oracles below
/// contradicted each other the moment the tier's two MariaDB legs differed — CI
/// pins the floating `mariadb:11` tag, which rolled from 11.4 to 11.8 and
/// brought fifteen functions with it. On 11.8 they are in `SQL_FUNCTIONS` and
/// the catalog was short by them (fifteen squiggles under correct SQL, which is
/// how this was found); on 10.11 they are absent and the catalog now carries
/// them, which without this list reads as fifteen invented names.
///
/// **Excused only where the endpoint really lacks them.** The tests below drop
/// a name from this list the moment the server in front of them reports it, so
/// the excuse cannot outlive the versions it was written for —
/// [`the_newer_names_are_reported_by_some_maria_db`] is what fails if the tier
/// ever stops seeing a server new enough to have any of them.
///
/// A tab on 10.11 *is* offered these fifteen and cannot call them. That is the
/// cheap direction and the same trade `intel::is_offered_builtin` already makes
/// for `ServerFlavour::Unknown`: one extra row in a popup, against a squiggle
/// under correct SQL on every newer server. Narrowing it would need a version
/// in `ServerFlavour`, which nothing else wants.
const NEWER_THAN_BASELINE: &[&str] = &[
    "format_bytes",
    "format_pico_time",
    "json_array_intersect",
    "json_key_value",
    "json_object_filter_keys",
    "json_object_to_array",
    "json_schema_valid",
    "kdf",
    "uuid_v4",
    "uuid_v7",
    "vec_distance",
    "vec_distance_cosine",
    "vec_distance_euclidean",
    "vec_fromtext",
    "vec_totext",
];

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

/// **The completeness direction, asked of the thing that actually goes wrong.**
///
/// [`every_function_the_server_reports_is_in_the_catalog`] asks
/// `SQL_FUNCTIONS`, whose own constant doc calls it "every builtin the parser
/// registers by name". It is not: a builtin registered natively and resolved by
/// its own path is in neither server view, and the whole OGC spatial family sat
/// in that gap — `ST_Length` squiggled under correct SQL on the default engine,
/// which is the exact failure this module exists to prevent, while every test
/// here was green. The other eighty were silent only because no catalogued name
/// happened to sit within a near-miss of them, which is luck rather than a
/// guard.
///
/// So this asks the **name source the server ships** — `mysql.help_topic`,
/// which both engines have — rather than a view written for a different
/// purpose, filters it through the parser, and then drives each survivor
/// through `intel::diagnostics`. That last step is the point: a name absent
/// from `FUNCTIONS` is only a *defect* when it produces a warning under correct
/// SQL, and this is the only test anywhere that composes the catalog with the
/// checker that reads it. Adding an unrelated entry that brings a real builtin
/// within edit distance fails here, which no membership test can see.
#[tokio::test(flavor = "multi_thread")]
async fn no_name_this_server_can_call_is_squiggled_under_correct_sql() {
    if !MARIADB.enabled() {
        endpoint::note_skipped(&MARIADB);
        return;
    }
    // Identifier-shaped help topics: the server's own index of everything it
    // documents, which is where the natively-registered builtins are to be
    // found. Not every topic is a function — `BEGIN`, `BIGINT`, `CALL` are in
    // there too — which is what the parser filter below is for.
    let topics = server_names(
        "SELECT `name` FROM mysql.help_topic WHERE `name` REGEXP '^[A-Za-z][A-Za-z0-9_]*$'",
        200,
    )
    .await;
    let mut candidates: Vec<String> = topics.into_iter().collect();
    candidates.sort();
    let uncallable = names_this_server_cannot_call(&candidates).await;
    let callable: Vec<&String> = candidates
        .iter()
        .filter(|n| !uncallable.contains(n))
        .collect();

    let catalog = schemaic_core::intel::Catalog::build(&[], None);
    let squiggled: Vec<String> = callable
        .iter()
        .filter(|n| {
            let sql = format!("SELECT {n}(a) FROM t");
            schemaic_core::intel::diagnostics(
                &sql,
                &catalog,
                schemaic_core::intel::SqlDialect::MySql,
            )
            .iter()
            .any(|d| d.message.contains("misspelled function"))
        })
        .map(|n| (*n).clone())
        .collect();

    assert!(
        squiggled.is_empty(),
        "{} can call {} name(s) that the editor squiggles as misspellings under \
         correct SQL — each one a warning on code that runs: {squiggled:?}",
        MARIADB.endpoint(),
        squiggled.len()
    );
    // The pass really ran. An empty candidate set, or a `help_topic` the server
    // ships unpopulated, would satisfy the assertion above having asked
    // nothing — which is the decoration this tier exists to avoid.
    assert!(
        callable.len() >= 150,
        "only {} callable name(s) reached the checker on {} — is \
         `mysql.help_topic` populated on this server?",
        callable.len(),
        MARIADB.endpoint()
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
    let unreported: Vec<String> = ours
        .iter()
        .filter(|n| !known.contains(*n))
        .filter(|n| !mysql_only().contains(n))
        // …and not a name this server is simply too old for. Keyed on what
        // *this* endpoint reports, so a newer one stops excusing it — see
        // `NEWER_THAN_BASELINE`.
        .filter(|n| !NEWER_THAN_BASELINE.contains(&n.as_str()))
        .cloned()
        .collect();

    // **The third oracle, and the only one that answers the real question.**
    // Neither view reports a builtin that is registered natively and resolved
    // by its own path — `ST_Area` is absent from `SQL_FUNCTIONS` while the same
    // server answers `SELECT st_area()` with "incorrect parameter count in the
    // call to **native function** 'st_area'". The module doc used to say the
    // union of the two views "is what the server claims"; it is not, and the
    // whole OGC spatial family sat in that gap. So a name the two views miss is
    // put to the parser, which is the question the editor actually asks — can
    // this server call this name.
    let unexplained = names_this_server_cannot_call(&unreported).await;

    assert!(
        unexplained.is_empty(),
        "`intel::FUNCTIONS` carries {} name(s) that neither \
         `information_schema.SQL_FUNCTIONS`, `information_schema.KEYWORDS` nor \
         {}'s own parser knows. Either they are MySQL 8's and belong in \
         MYSQL_ONLY, or they are invented and would suppress a genuine typo \
         warning: {unexplained:?}",
        unexplained.len(),
        MARIADB.endpoint()
    );

    // **And the `KEYWORDS` excuse is put to the parser too** (`S6-L6-04`).
    // Reserved-word-ness does not imply callability, which is the converse of
    // the sentence `NOT_CALLABLE`'s own doc argues — and this half was using it
    // as if it were sound: 696 keywords, of which 681 are not in
    // `SQL_FUNCTIONS`, standing as a blanket excuse for the 44 catalog names
    // that need one. Add `f("TABLESPACE", …)` to `intel::FUNCTIONS` and the
    // assertion above never sees it, while the popup offers a name that
    // completes to a syntax error and the checker stops squiggling
    // `tablespace(` for whoever meant something else.
    //
    // A name with its own grammar rule answers `1064` to `NAME(1,2)` because
    // `(1,2)` is not its syntax — measured: `CAST`, `COUNT`, `IF`, `PARTITION`
    // and `SAVEPOINT` all do. An invented one answers `1630`: measured, that is
    // exactly what `TABLESPACE` gives. So the two *are* distinguishable here,
    // which is what makes this checkable at all.
    let by_keyword_only: Vec<String> = ours
        .iter()
        .filter(|n| !functions.contains(*n) && keywords.contains(*n))
        .cloned()
        .collect();
    let invented = names_the_server_has_never_heard_of(&by_keyword_only).await;
    assert!(
        invented.is_empty(),
        "{} of `intel::FUNCTIONS`' names are excused only by being reserved \
         words on {}, and its parser says it has no such function — so they are \
         invented, and each one silently suppresses a genuine typo warning \
         while completing to a syntax error: {invented:?}",
        invented.len(),
        MARIADB.endpoint()
    );
    assert!(
        !by_keyword_only.is_empty(),
        "no catalog name is excused by `KEYWORDS` on {}, so the check above \
         asked nothing",
        MARIADB.endpoint()
    );

    // **The partition adds up, computed rather than written down.** The names
    // `SQL_FUNCTIONS` misses are of three kinds: those with their own grammar
    // rule, which are MariaDB's reserved words and so in `KEYWORDS`; MySQL 8's,
    // which `MYSQL_ONLY` names; and the natively-registered ones neither view
    // reports, which only the parser can vouch for. The module doc used to
    // state this as "39 names covered by KEYWORDS, and the five left over",
    // which does not add up to the set it partitions and was never computed by
    // anything; and it was a *two*-way split of a three-way set.
    let by_keyword: Vec<&String> = ours
        .iter()
        .filter(|n| !functions.contains(*n) && keywords.contains(*n))
        .collect();
    let by_mysql_only: Vec<&String> = ours
        .iter()
        .filter(|n| !known.contains(*n) && mysql_only().contains(n))
        .collect();
    // The fourth part: names this endpoint is too old for. Zero on a server new
    // enough to report them all, which is why the assertion below counts it
    // rather than assuming it.
    let by_version: Vec<&String> = ours
        .iter()
        .filter(|n| !known.contains(*n) && !mysql_only().contains(n))
        .filter(|n| NEWER_THAN_BASELINE.contains(&n.as_str()))
        .collect();
    let by_parser = unreported.len();
    let not_in_functions = ours.iter().filter(|n| !functions.contains(*n)).count();
    assert_eq!(
        by_keyword.len() + by_mysql_only.len() + by_version.len() + by_parser,
        not_in_functions,
        "the four parts of the excuse do not add up to what `SQL_FUNCTIONS` \
         leaves out on {}: {} by KEYWORDS + {} by MYSQL_ONLY + {} too new for \
         this server + {} by the parser against {} unreported",
        MARIADB.endpoint(),
        by_keyword.len(),
        by_mysql_only.len(),
        by_version.len(),
        by_parser,
        not_in_functions
    );
    assert!(
        !by_keyword.is_empty() && !by_mysql_only.is_empty() && by_parser > 0,
        "one part of the partition is empty on {} ({} / {} / {}), so the sum \
         above is not measuring a partition at all",
        MARIADB.endpoint(),
        by_keyword.len(),
        by_mysql_only.len(),
        by_parser
    );
}

/// Does this error mean the server will not accept the name as a call?
///
/// **One rule, because two tests were asking it and only one of them had it
/// right.** `1305` and `1630` are both "no such function" — MariaDB uses the
/// second for a name whose shape reaches builtin resolution and finds nothing
/// (`AUTO_INCREMENT`, `GOTO`, `JSON_TABLE`, `MERGE`, `RESTART` all answer it),
/// and reading only `1305` credits the server with five functions it does not
/// have. `1064` is [`NOT_CALLABLE`]'s rule and the one that separates a builtin
/// from a *type* or a reserved word: `mysql.help_topic` indexes `DATETIME`,
/// `BEGIN` and `CALL` alongside the functions, and MySQL 8.4 answers it for
/// `SRID` and `CONTAINS`, which it keeps as reserved words after removing the
/// functions. A name that cannot be typed as a call cannot be squiggled as one
/// either — and must not be offered, because completing it produces a syntax
/// error.
///
/// Anything else — wrong arity, wrong types — means the server knows the name.
fn is_not_callable(err: &str) -> bool {
    ["1305", "1630", "1064"].iter().any(|c| err.contains(c))
}

/// Which of `names` this server will not accept as a call.
///
/// **The parser is an oracle, and the better one**, for the reason
/// [`each_server_is_only_credited_with_the_builtins_it_really_has`] gives: it
/// answers the question the editor actually asks. [`is_not_callable`] is the
/// rule.
async fn names_this_server_cannot_call(names: &[String]) -> Vec<String> {
    probe(names, is_not_callable).await
}

/// Which of `names` this server says it has **no such function** for — `1305`
/// or `1630`, and deliberately *not* `1064`.
///
/// The narrower question, for the names [`over_listing`] excuses by their being
/// reserved words. A builtin with its own grammar rule answers `1064` to
/// `NAME(1,2)` because that is not its syntax, so reading `1064` as absence
/// would condemn `CAST`, `COUNT` and `IF`. An *invented* name answers `1630`.
async fn names_the_server_has_never_heard_of(names: &[String]) -> Vec<String> {
    probe(names, |e| e.contains("1305") || e.contains("1630")).await
}

/// Run `SELECT <name>(1,2)` for each name and keep those whose error `pick`
/// accepts.
///
/// One pinned [`Session`] rather than a connection per name, scoped to `mysql`
/// so an unqualified unknown name is resolved as a stored function there rather
/// than answered with `1046 No database selected`, which would report every
/// name as present.
async fn probe(names: &[String], pick: fn(&str) -> bool) -> Vec<String> {
    use schemaic_db::session::Session;

    if names.is_empty() {
        return Vec::new();
    }
    let session = Session::open(&MARIADB.base_db(), Some("mysql"))
        .await
        .unwrap_or_else(|e| panic!("{}: could not pin a session: {e}", MARIADB.endpoint()));
    let mut absent = Vec::new();
    for name in names {
        let sql = format!("SELECT {name}(1,2)");
        if let Err(e) = session
            .fetch_query(&sql, 1, CancellationToken::new())
            .await
            .result
            && pick(&format!("{e}"))
        {
            absent.push(name.clone());
        }
    }
    session.close().await;
    absent
}

/// **The version excuse describes a real boundary, and is spent where it is
/// claimed.**
///
/// [`NEWER_THAN_BASELINE`] is the one list here that no single endpoint can
/// confirm — it is about the difference between two MariaDB versions, and the
/// tier sees one at a time. What *is* checkable from either side is that the
/// list has not gone stale in the two ways that matter: a name on it that the
/// catalog does not carry excuses nothing at all, and a name this endpoint
/// **does** report must be callable here, or the excuse is covering an
/// unrelated defect.
///
/// The second half is what makes the list self-limiting. On a server new enough
/// to report them, every name is exercised against the parser and the excuse
/// does no work; on an older one it does all of it. Either way a name that
/// stops being a version story fails here rather than sitting in the list.
#[tokio::test(flavor = "multi_thread")]
async fn the_newer_names_are_a_version_boundary_and_not_a_dumping_ground() {
    if !MARIADB.enabled() {
        endpoint::note_skipped(&MARIADB);
        return;
    }
    let ours = catalog_names();
    let missing: Vec<&&str> = NEWER_THAN_BASELINE
        .iter()
        .filter(|n| !ours.contains(**n))
        .collect();
    assert!(
        missing.is_empty(),
        "`NEWER_THAN_BASELINE` names {} function(s) `intel::FUNCTIONS` does not \
         carry, so they excuse nothing and the list has drifted: {missing:?}",
        missing.len()
    );

    // What this server actually reports of them — nothing on 10.11, all of them
    // on 11.8.
    let reported = server_names(FUNCTION_ORACLE, 200).await;
    let here: Vec<String> = NEWER_THAN_BASELINE
        .iter()
        .filter(|n| reported.contains(**n))
        .map(|n| (*n).to_string())
        .collect();
    let uncallable = names_this_server_cannot_call(&here).await;
    assert!(
        uncallable.is_empty(),
        "{} reports {:?} in `SQL_FUNCTIONS` and will not call them, so they are \
         not a version story — `NEWER_THAN_BASELINE` is covering something \
         else",
        MARIADB.endpoint(),
        uncallable
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
/// name. `SELECT <name>(1,2)` comes back `ERROR 1305` or `1630` when the server
/// has no such function, and with some other error (wrong arity, wrong types)
/// when it has one.
///
/// **`1064` is the third answer, and reading it as either of the other two is
/// wrong.** A builtin with its own grammar rule answers it because `(1,2)` is
/// not its syntax — `CAST`, `COUNT`, `IF` — and so does a name the server
/// removed but kept reserved: MySQL 8.4 does that with `SRID` and `CONTAINS`,
/// and no arity makes either parse while `COUNT(1)` does. Calling it "present"
/// made this test demand MySQL be offered two names it cannot type; calling it
/// "absent" would make it demand MariaDB's `COUNT` be withheld. So the walk
/// below has three outcomes and the `stale` half skips the inconclusive ones.
///
/// **Both directions, from one pass.** What MySQL 8 lacks must be exactly
/// `intel`'s `MARIADB_ONLY`, and the same pass re-measures [`MYSQL_ONLY`] on
/// MariaDB — so the two lists that decide what each tab is offered are checked
/// against the two servers rather than against each other.
///
/// One pinned [`Session`] rather than a connection per name: this is the
/// documented exception to one-connection-per-operation, and a `Db::fetch_query`
/// per catalog entry would open one apiece.
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

        // **Three outcomes, not two.** `1305`/`1630` is "no such function".
        // Anything else is "has it, called wrongly" — *except* `1064`, which
        // this probe cannot read either way: `COUNT(1,2)`, `CAST(1,2)`,
        // `EXTRACT(1,2)` and the whole grammar-rule family answer it because
        // `(1,2)` is not their syntax, and so do `SRID(…)` and `CONTAINS(…)` on
        // MySQL 8.4, which removed the functions and kept the words reserved.
        // Measured: `SRID(1)`, `SRID(ST_GeomFromText(…))` and `CONTAINS(1)` are
        // all 1064 there, so no arity makes them parse — and `COUNT(1)` does.
        // Calling that "present" made the test demand MySQL be offered two
        // names it cannot type; calling it "absent" makes it demand MariaDB's
        // `COUNT` be withheld. So it is neither, and the assertions below say
        // so.
        let mut absent: Vec<&str> = Vec::new();
        let mut inconclusive: Vec<&str> = Vec::new();
        for f in FUNCTIONS {
            let sql = format!("SELECT {}(1,2)", f.name);
            if let Err(e) = session
                .fetch_query(&sql, 1, CancellationToken::new())
                .await
                .result
            {
                let e = format!("{e}");
                if e.contains("1305") || e.contains("1630") {
                    absent.push(f.name);
                } else if e.contains("1064") {
                    inconclusive.push(f.name);
                }
            }
        }
        session.close().await;
        let inconclusive: HashSet<&str> = inconclusive.into_iter().collect();

        let expected: HashSet<&str> = expected.iter().copied().collect();
        let absent: HashSet<&str> = absent.into_iter().collect();

        // A name the *engine* has and this *version* does not is excused here
        // and nowhere else — see `NEWER_THAN_BASELINE`, which also records why
        // over-offering is the direction to accept.
        let unlisted: Vec<&&str> = absent
            .difference(&expected)
            .filter(|n| !NEWER_THAN_BASELINE.contains(&n.to_ascii_lowercase().as_str()))
            .collect();
        assert!(
            unlisted.is_empty(),
            "{} has no such function, and nothing in `intel` withholds it — so \
             a tab on this server is offered {} name(s) it cannot call: \
             {unlisted:?}",
            target.endpoint(),
            unlisted.len()
        );
        let stale: Vec<&&str> = expected
            .difference(&absent)
            .filter(|n| !inconclusive.contains(**n))
            .collect();
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
