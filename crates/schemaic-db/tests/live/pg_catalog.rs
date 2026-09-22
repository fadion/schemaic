//! `pg_builtins::PG_FUNCTIONS` against the PostgreSQL the tier is pointed at.
//!
//! **The catalog is data, and data is what goes stale silently.** It decides
//! whether the editor calls a function name a typo, so a name the engine really
//! has and the list really lacks is a squiggle under correct SQL — the exact
//! failure that switched the typo checker off for this engine in the first
//! place. `pg_catalog` is the server's own answer to the same question, so it is
//! the thing to ask rather than anybody's memory of the documentation.
//!
//! **This is the one place in the tier that names an engine**, and it does so
//! because the thing under test is one engine's data file rather than a claim
//! about how the DB layer behaves. [`crate::suite`]'s rule — assert what is true
//! of every target, read the differences off [`Target`] — applies to the suite,
//! which is written once and run three times; there is no version of "is
//! PostgreSQL's builtin list complete" that MariaDB can answer. `sqlite_catalog`
//! is the same test for SQLite and lives in the pure tier, because its oracle is
//! a linked library where this one is a server.
//!
//! It is also **not** in the `live_suite!` macro, for the same reason: the macro
//! expands one function into a test per leg.

use std::collections::HashSet;

use schemaic_core::pg_builtins::{PG_FUNCTIONS, PG_SUGGESTED};
use tokio_util::sync::CancellationToken;

use crate::endpoint::{self, POSTGRES};

/// The query `pg_builtins`' module doc records as the generated half's source,
/// minus the two columns only the generator needed.
///
/// Written out here rather than shared with anything, because a shared constant
/// would let one edit move both the catalog and the oracle that checks it.
const ORACLE: &str = "SELECT DISTINCT p.proname \
                        FROM pg_proc p JOIN pg_namespace n ON n.oid = p.pronamespace \
                       WHERE n.nspname = 'pg_catalog' \
                         AND p.prokind IN ('f', 'a', 'w') \
                         AND p.proname ~ '^[a-z][a-z0-9_]*$'";

/// The cut `PG_SUGGESTED` records: every builtin **except** the ones that exist
/// to implement something else.
///
/// The exclusion is read off the catalogs that point *at* a function — an
/// operator's implementation or selectivity estimator, an index support
/// function, any of `pg_aggregate`'s support columns, a type's I/O, typmod,
/// analyze or subscript routine, a cast, a range's canonical/subdiff, a language
/// or access-method handler — plus anything trafficking in `internal`/`cstring`
/// or returning a handler pseudo-type. Nothing here is a name; that is the whole
/// point, and it is why this can be re-asked rather than re-remembered.
///
/// Written out here rather than shared with `pg_builtins`, for the reason
/// [`ORACLE`] is: a shared constant would let one edit move both the subset and
/// the check on it.
const SUGGEST_ORACLE: &str = "\
WITH all_f AS ( \
  SELECT p.oid, p.proname FROM pg_proc p JOIN pg_namespace n ON n.oid = p.pronamespace \
   WHERE n.nspname = 'pg_catalog' AND p.prokind IN ('f','a','w') \
     AND p.proname ~ '^[a-z][a-z0-9_]*$' \
), internal AS ( \
  SELECT oprcode AS oid FROM pg_operator \
  UNION SELECT oprrest FROM pg_operator UNION SELECT oprjoin FROM pg_operator \
  UNION SELECT amproc FROM pg_amproc \
  UNION SELECT aggtransfn FROM pg_aggregate UNION SELECT aggfinalfn FROM pg_aggregate \
  UNION SELECT aggcombinefn FROM pg_aggregate UNION SELECT aggserialfn FROM pg_aggregate \
  UNION SELECT aggdeserialfn FROM pg_aggregate UNION SELECT aggmtransfn FROM pg_aggregate \
  UNION SELECT aggminvtransfn FROM pg_aggregate UNION SELECT aggmfinalfn FROM pg_aggregate \
  UNION SELECT typinput FROM pg_type UNION SELECT typoutput FROM pg_type \
  UNION SELECT typreceive FROM pg_type UNION SELECT typsend FROM pg_type \
  UNION SELECT typmodin FROM pg_type UNION SELECT typmodout FROM pg_type \
  UNION SELECT typanalyze FROM pg_type UNION SELECT typsubscript FROM pg_type \
  UNION SELECT castfunc FROM pg_cast \
  UNION SELECT rngcanonical FROM pg_range UNION SELECT rngsubdiff FROM pg_range \
  UNION SELECT lanplcallfoid FROM pg_language UNION SELECT laninline FROM pg_language \
  UNION SELECT lanvalidator FROM pg_language \
  UNION SELECT amhandler FROM pg_am \
), typed AS ( \
  SELECT p.oid FROM pg_proc p \
   WHERE p.prorettype IN ('cstring'::regtype,'internal'::regtype,'trigger'::regtype, \
         'event_trigger'::regtype,'language_handler'::regtype,'fdw_handler'::regtype, \
         'index_am_handler'::regtype,'table_am_handler'::regtype,'tsm_handler'::regtype) \
      OR EXISTS (SELECT 1 FROM unnest(p.proargtypes) t(x) \
                  WHERE x IN ('cstring'::regtype,'internal'::regtype)) \
) \
SELECT DISTINCT proname FROM all_f a \
 WHERE a.oid NOT IN (SELECT oid FROM internal WHERE oid IS NOT NULL AND oid <> 0) \
   AND a.oid NOT IN (SELECT oid FROM typed)";

/// The call forms PostgreSQL's grammar implements without a `pg_proc` row, which
/// is why the server cannot report them and [`over_listing`] must account for
/// them by hand. The second spelling of the list in `pg_builtins`, deliberately:
/// a name that drifts into one and not the other is what this catches.
const GRAMMAR_ONLY: &[&str] = &[
    "cast",
    "coalesce",
    "current_time",
    "current_timestamp",
    "greatest",
    "grouping",
    "json",
    "json_array",
    "json_arrayagg",
    "json_objectagg",
    "least",
    "localtime",
    "localtimestamp",
    "nullif",
    "row",
    "treat",
    "trim",
    "xmlconcat",
    "xmlelement",
    "xmlforest",
    "xmlparse",
    "xmlpi",
    "xmlroot",
    "xmlserialize",
];

/// The server knows a name this catalog does not — a false positive waiting for
/// whoever types it.
#[tokio::test(flavor = "multi_thread")]
async fn every_function_the_server_reports_is_in_the_catalog() {
    if !POSTGRES.enabled() {
        endpoint::note_skipped(&POSTGRES);
        return;
    }
    let server = server_function_names().await;
    let ours = catalog_names();

    let missing: Vec<&str> = server
        .iter()
        .map(String::as_str)
        .filter(|n| !ours.contains(*n))
        .collect();

    assert!(
        missing.is_empty(),
        "{} reports {} function(s) `PG_FUNCTIONS` does not carry, so each would \
         be squiggled as a misspelling of whatever it happens to resemble. \
         Re-run the query in `pg_builtins`' module docs and regenerate the file \
         — the three rules for turning a row into an entry are written out \
         beside it, and `every_generated_cell_is_what_the_recipe_produces` says \
         whether you followed them: {missing:?}",
        POSTGRES.endpoint(),
        missing.len()
    );
}

/// The catalog carries a name the server does not, and this test says which
/// kinds are allowed to be there.
///
/// Unlike SQLite's `over_listing`, the overhang here is *known and finite*: the
/// grammar's own call forms, which have no `pg_proc` row to be reported from.
/// Anything else is either a name invented by hand — which silently suppresses a
/// genuine typo warning — or one this server's major version has dropped, and
/// both are worth the sentence.
#[tokio::test(flavor = "multi_thread")]
async fn over_listing() {
    if !POSTGRES.enabled() {
        endpoint::note_skipped(&POSTGRES);
        return;
    }
    let server = server_function_names().await;
    let unexplained: Vec<String> = catalog_names()
        .into_iter()
        .filter(|n| !server.iter().any(|s| s == n))
        .filter(|n| !GRAMMAR_ONLY.contains(&n.as_str()))
        .collect();

    assert!(
        unexplained.is_empty(),
        "`PG_FUNCTIONS` carries {} name(s) {} does not report and this test \
         cannot account for. Either they are the grammar's and belong in \
         GRAMMAR_ONLY, or they are invented and would suppress a genuine typo \
         warning: {unexplained:?}",
        unexplained.len(),
        POSTGRES.endpoint()
    );
}

/// Every name in [`GRAMMAR_ONLY`] really is in the catalog, so the allowance
/// [`over_listing`] grants cannot outlive what it was granted for.
///
/// Without this, deleting `trim` from `PG_FUNCTIONS` — the entry a corpus test
/// had to find in the first place — leaves both tests above green.
#[tokio::test(flavor = "multi_thread")]
async fn the_grammar_forms_are_in_the_catalog() {
    if !POSTGRES.enabled() {
        endpoint::note_skipped(&POSTGRES);
        return;
    }
    let ours = catalog_names();
    let missing: Vec<&&str> = GRAMMAR_ONLY
        .iter()
        .filter(|n| !ours.contains(**n))
        .collect();
    assert!(
        missing.is_empty(),
        "`over_listing` excuses these from the server's answer, and \
         `PG_FUNCTIONS` does not carry them either, so nothing holds them at \
         all: {missing:?}"
    );
}

/// `PG_SUGGESTED` is exactly what [`SUGGEST_ORACLE`] still answers, plus
/// [`GRAMMAR_ONLY`].
///
/// The subset autocomplete offers is generated, like the catalog it is drawn
/// from, so the thing that can rot is the same: a PostgreSQL release adds a
/// function and the popup never learns it, or reclassifies one and the popup
/// goes on offering plumbing. Asking the server is the only way to see either.
///
/// **The `GRAMMAR_ONLY` union is the half a query cannot cover**, and is why the
/// subset is not simply the query's output: `coalesce`, `cast`, `trim`,
/// `greatest` and `least` have no `pg_proc` row, so a server-derived list drops
/// the five most typed names in it while looking like a tidy-up.
#[tokio::test(flavor = "multi_thread")]
async fn the_offered_subset_is_still_what_the_filter_answers() {
    if !POSTGRES.enabled() {
        endpoint::note_skipped(&POSTGRES);
        return;
    }
    let rs = POSTGRES
        .base_db()
        .fetch_query(None, SUGGEST_ORACLE, 10_000, CancellationToken::new())
        .await
        .unwrap_or_else(|e| {
            panic!(
                "live tier could not run the suggestion filter on {}: {e}",
                POSTGRES.endpoint()
            )
        });
    let mut expected: HashSet<String> = (0..rs.row_count())
        .filter_map(|r| rs.cell(r, 0).map(|c| c.text().to_ascii_lowercase()))
        .collect();
    // Same floor, same reason as `server_function_names`: an empty answer would
    // make the two comparisons below vacuous in the direction that matters.
    assert!(
        expected.len() > 500,
        "only {} name(s) survived the filter, so this oracle is not answering",
        expected.len()
    );
    expected.extend(GRAMMAR_ONLY.iter().map(|n| (*n).to_string()));

    let ours: HashSet<String> = PG_SUGGESTED.iter().map(|n| (*n).to_string()).collect();

    let missing: Vec<&String> = expected.difference(&ours).collect();
    assert!(
        missing.is_empty(),
        "{} offers {} name(s) `PG_SUGGESTED` does not, so autocomplete will \
         never suggest them. Re-run `SUGGEST_ORACLE` above and regenerate the \
         list: {missing:?}",
        POSTGRES.endpoint(),
        missing.len()
    );
    let extra: Vec<&String> = ours.difference(&expected).collect();
    assert!(
        extra.is_empty(),
        "`PG_SUGGESTED` carries {} name(s) the filter no longer keeps, so the \
         popup is offering this server's plumbing: {extra:?}",
        extra.len()
    );
}

/// The full generated row — name, identity arguments, description — which is
/// what [`every_generated_cell_is_what_the_recipe_produces`] rebuilds from.
///
/// [`ORACLE`]'s three columns rather than its one, spelled separately for
/// [`ORACLE`]'s own reason: a shared constant would let one edit move both the
/// catalog and the check on it.
const CELL_ORACLE: &str = "SELECT DISTINCT ON (p.proname) \
                                  p.proname, \
                                  pg_get_function_identity_arguments(p.oid), \
                                  coalesce(obj_description(p.oid, 'pg_proc'), '') \
                             FROM pg_proc p JOIN pg_namespace n ON n.oid = p.pronamespace \
                            WHERE n.nspname = 'pg_catalog' \
                              AND p.prokind IN ('f', 'a', 'w') \
                              AND p.proname ~ '^[a-z][a-z0-9_]*$' \
                            ORDER BY p.proname, p.pronargs, p.oid";

/// **The 5,412 cells that had no oracle at all.**
///
/// Both live tests and both pure tests compared *names*; `signature` and
/// `summary` are what the completion popup renders and what `rank` puts in a
/// suggestion's detail, and nothing anywhere checked their content. Meanwhile
/// this file told the next maintainer to "re-run the query in `pg_builtins`'
/// module docs and regenerate the file" — and doing that literally would have
/// rewritten 156 signatures and added twelve rows, green the whole way, because
/// the query as recorded was missing the `proname` filter and
/// `pg_get_function_identity_arguments` includes `OUT` parameters on PG 16.
///
/// The data was never drifted; the *documentation of how to make it* was. So
/// this pins the rule the data actually follows — see `pg_builtins`' module doc,
/// where all three parts are now written out — by rebuilding every cell from the
/// server and comparing. Measured at 16.15: 0 deviations in either direction.
///
/// The hand-written grammar forms (`cast`, `coalesce`, the `xml*` constructors)
/// are not reached, and correctly: they have no `pg_proc` row, so the server
/// returns nothing to rebuild them from. `every_function_the_server_reports_is_
/// in_the_catalog` is what covers the other direction.
#[tokio::test(flavor = "multi_thread")]
async fn every_generated_cell_is_what_the_recipe_produces() {
    if !POSTGRES.enabled() {
        endpoint::note_skipped(&POSTGRES);
        return;
    }
    /// Rule 1: identity arguments with every `OUT ` parameter dropped.
    fn without_out_params(args: &str) -> String {
        let mut kept: Vec<&str> = Vec::new();
        let (mut depth, mut start) = (0i32, 0usize);
        for (i, ch) in args.char_indices() {
            match ch {
                '(' | '[' => depth += 1,
                ')' | ']' => depth -= 1,
                ',' if depth == 0 => {
                    kept.push(&args[start..i]);
                    start = i + 1;
                }
                _ => {}
            }
        }
        if !args[start..].trim().is_empty() {
            kept.push(&args[start..]);
        }
        kept.iter()
            .map(|a| a.trim())
            .filter(|a| !a.starts_with("OUT "))
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// Rules 1 and 2 together.
    fn signature(name: &str, args: &str) -> String {
        let full = format!("{name}({})", without_out_params(args));
        if full.len() > 72 {
            format!("{name}(…)")
        } else {
            full
        }
    }

    /// Rule 3.
    fn summary(desc: &str) -> String {
        let d = desc.trim();
        let mut cs = d.chars();
        match cs.next() {
            Some(c) => c.to_uppercase().collect::<String>() + cs.as_str(),
            None => "No description in `pg_proc`".to_string(),
        }
    }

    let rs = POSTGRES
        .base_db()
        .fetch_query(None, CELL_ORACLE, 10_000, CancellationToken::new())
        .await
        .unwrap_or_else(|e| {
            panic!(
                "live tier could not read pg_catalog on {}: {e}\nstatement: {CELL_ORACLE}",
                POSTGRES.endpoint()
            )
        });

    let ours: std::collections::HashMap<&str, &schemaic_core::intel::SqlFunction> =
        PG_FUNCTIONS.iter().map(|f| (f.name, f)).collect();

    let mut wrong: Vec<String> = Vec::new();
    let mut checked = 0usize;
    for r in 0..rs.row_count() {
        let cell = |c: usize| {
            rs.cell(r, c)
                .map(|v| v.text().to_string())
                .unwrap_or_default()
        };
        let (name, args, desc) = (cell(0), cell(1), cell(2));
        let Some(entry) = ours.get(name.as_str()) else {
            // Absence is `every_function_the_server_reports_is_in_the_catalog`'s
            // to report, with its own message.
            continue;
        };
        checked += 1;
        let want = signature(&name, &args);
        if entry.signature != want {
            wrong.push(format!(
                "{name}: signature {:?} != {want:?}",
                entry.signature
            ));
        }
        let want = summary(&desc);
        if entry.summary != want {
            wrong.push(format!("{name}: summary {:?} != {want:?}", entry.summary));
        }
    }

    assert!(
        wrong.is_empty(),
        "{} generated cell(s) on {} are not what the recipe in `pg_builtins`' \
         module doc produces, so regenerating the file the way that doc says \
         would change them. Either the file was hand-edited or the rules have \
         moved — fix whichever is wrong, and keep the doc and the data \
         together:\n  {}",
        wrong.len(),
        POSTGRES.endpoint(),
        wrong
            .iter()
            .take(20)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n  ")
    );
    assert!(
        checked > 2_000,
        "only {checked} generated entr(ies) were compared on {}, so this oracle \
         is not answering",
        POSTGRES.endpoint()
    );
}

/// Every function name the configured server reports, lower-cased.
async fn server_function_names() -> Vec<String> {
    let rs = POSTGRES
        .base_db()
        .fetch_query(None, ORACLE, 10_000, CancellationToken::new())
        .await
        .unwrap_or_else(|e| {
            panic!(
                "live tier could not read pg_catalog on {}: {e}\nstatement: {ORACLE}",
                POSTGRES.endpoint()
            )
        });
    let names: Vec<String> = (0..rs.row_count())
        .filter_map(|r| rs.cell(r, 0).map(|c| c.text().to_ascii_lowercase()))
        .collect();
    // The row cap is 10,000 and PostgreSQL 16 answers with 2,682, so a truncated
    // result is not the worry — an *empty* one is. A query that returned nothing
    // would pass `every_function_the_server_reports_is_in_the_catalog`
    // vacuously, which is the decoration this tier exists to avoid.
    assert!(
        names.len() > 1_000,
        "only {} names came back from pg_catalog, so this oracle is not \
         answering and the test above would pass having asserted nothing",
        names.len()
    );
    names
}

fn catalog_names() -> HashSet<String> {
    PG_FUNCTIONS
        .iter()
        .map(|f| f.name.to_ascii_lowercase())
        .collect()
}

/// **An extension's functions are callable names the tree deliberately omits,
/// and the typo checker must still know them.**
///
/// `pg::routine_filter` excludes `deptype = 'e'` on purpose — PostGIS alone
/// installs ~1,000 functions into `public`, and the Functions folder would be a
/// wall of `st_*` with the user's own routines lost inside it. But
/// `intel::function_typo_checks` exempts what the catalog's `known_idents`
/// holds, and that set was built from what the tree lists — so an extension
/// function was not merely absent from the tree, it was **squiggled as a
/// misspelling under correct SQL**.
///
/// Measured on PG 16.15 before the fix, with the six extensions below
/// installed: **19 of the 196 extension-owned names** came back "looks like a
/// misspelled function" — `earth_distance`, `icount`, `sort`, `tconvert` and
/// fifteen `citext_*`. `sort` and `icount` are `intarray`'s, and are exactly
/// the short single-word shape `an_ordinary_user_function_survives_the_pg_
/// catalog` had no sample of.
///
/// **A server is the only thing that can answer this**, which is why it is
/// here rather than beside the unit test: the unit half asserts that a name in
/// `DbSchema::extension_routines` is exempt, and what only a real PostgreSQL
/// can say is that `fetch_schema` actually *puts* its extensions' names there.
/// The extensions are created in the tier's own scratch database and go away
/// with it.
#[tokio::test(flavor = "multi_thread")]
async fn an_installed_extensions_functions_are_names_the_checker_knows() {
    use schemaic_core::intel::{Catalog, Severity, diagnostics};

    if !POSTGRES.enabled() {
        endpoint::note_skipped(&POSTGRES);
        return;
    }
    let scratch = crate::scratch::Scratch::create(&POSTGRES, "pgext").await;
    // Six that ship with a stock PostgreSQL and between them cover the shapes
    // that near-miss: `citext`'s comparison operators, `intarray`'s short
    // `sort`/`icount`, `earthdistance`'s `earth_distance`, `tablefunc`'s
    // `tconvert`.
    for ext in [
        "citext",
        "intarray",
        "cube",
        "earthdistance",
        "tablefunc",
        "hstore",
    ] {
        scratch
            .exec(&format!("CREATE EXTENSION IF NOT EXISTS \"{ext}\""))
            .await;
    }

    let schema = scratch
        .db
        .fetch_schema(&scratch.database, CancellationToken::new())
        .await
        .unwrap_or_else(|e| panic!("fetch_schema on {}: {e}", POSTGRES.endpoint()));

    // The premise: the read really found them. An empty list would make every
    // assertion below pass having asserted nothing — the decoration this tier
    // exists to avoid.
    assert!(
        schema.extension_routines.len() > 100,
        "only {} extension function name(s) came back, so `fetch_schema` is \
         not reading them and the rest of this test is vacuous",
        schema.extension_routines.len()
    );
    // …and they are names only, not routines the tree would offer to edit.
    for name in ["sort", "icount", "earth_distance"] {
        assert!(
            schema
                .extension_routines
                .iter()
                .any(|n| n.eq_ignore_ascii_case(name)),
            "{name} is installed on this server and is not in extension_routines"
        );
        assert!(
            !schema
                .routines
                .iter()
                .any(|r| r.name.eq_ignore_ascii_case(name)),
            "{name} reached the browse list, which `routine_filter` exists to \
             keep it out of"
        );
    }

    // And not one of them is called a misspelling.
    let cat = Catalog::build(
        &[(scratch.database.as_str(), &schema)],
        Some(&scratch.database),
    );
    let squiggled: Vec<&String> = schema
        .extension_routines
        .iter()
        .filter(|n| {
            let sql = format!("SELECT {n}(a) FROM t");
            diagnostics(&sql, &cat, schemaic_core::intel::SqlDialect::Postgres)
                .iter()
                .any(|d| {
                    d.severity == Severity::Warning && d.message.contains("misspelled function")
                })
        })
        .collect();
    assert!(
        squiggled.is_empty(),
        "{} of this server's {} extension function names are squiggled as \
         misspellings under correct SQL: {squiggled:?}",
        squiggled.len(),
        schema.extension_routines.len()
    );

    scratch.teardown().await;
}
