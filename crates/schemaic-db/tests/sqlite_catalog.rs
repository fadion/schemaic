//! `intel::SQLITE_FUNCTIONS` against the SQLite this workspace actually links.
//!
//! **The catalog is data, and data is what goes stale silently.** It decides
//! whether the editor calls a function name a typo, so a name the engine really
//! has and the list really lacks is a squiggle under correct SQL — the exact
//! failure that switched the typo checker off for two engines in the first
//! place. `pragma_function_list` is the engine's own answer to the same
//! question, so it is the thing to ask rather than anybody's memory of the
//! documentation. Writing the list by hand found 23 names only this test knew
//! were missing, the R-tree and FTS families among them.
//!
//! **It lives here rather than in `schemaic-core`, next to the catalog it
//! guards, because only this crate links rusqlite** — and it needs no server, no
//! file and no network, which is why an in-memory SQLite is allowed where the
//! rest of the pure tier's rule would otherwise forbid a live engine.
//!
//! The check is deliberately **one-directional**; see `over_listing` for why the
//! other direction is not a failure.

/// The engine knows a name this catalog does not — a false positive waiting for
/// whoever types it.
#[test]
fn every_function_the_engine_reports_is_in_the_catalog() {
    let engine = engine_function_names();
    let ours = catalog_names();

    let missing: Vec<&str> = engine
        .iter()
        .map(String::as_str)
        // `sqlite_rename_*` is the rename machinery ALTER TABLE drives, not
        // callable API; `SQLITE_FUNCTIONS`' doc says why offering those names to
        // the near-miss test would be worse than omitting them.
        .filter(|n| !n.starts_with("sqlite_rename"))
        // `->` and `->>` are operators. The checker only ever looks at a word
        // followed by `(`, so a name that is not an identifier cannot reach it.
        .filter(|n| n.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'))
        .filter(|n| !ours.iter().any(|o| o == n))
        .collect();

    assert!(
        missing.is_empty(),
        "the linked SQLite reports {} function(s) the catalog does not carry, so \
         each would be squiggled as a misspelling of whatever it happens to \
         resemble: {missing:?}",
        missing.len()
    );
}

/// The reverse listing is **not** a failure, and this test says so rather than
/// leaving the asymmetry to be discovered and "fixed".
///
/// A `.db` is a file other tools open, and the catalog answers for SQLite rather
/// than for this build of it. The math functions
/// (`SQLITE_ENABLE_MATH_FUNCTIONS`) and `sqlite_offset` are compiled out here,
/// and no table-valued function — `json_each`, `json_tree` — appears in
/// `pragma_function_list` at all. Squiggling `sqrt(x)`, which is correct SQL
/// almost everywhere, to describe a local compile flag is the false positive the
/// catalog exists to avoid. So this asserts only that the overhang stays
/// *explicable*: every name in it is one of the families named above.
#[test]
fn over_listing() {
    let engine = engine_function_names();
    let unexplained: Vec<String> = catalog_names()
        .into_iter()
        .filter(|n| !engine.iter().any(|e| e == n))
        .filter(|n| !MATH.contains(&n.as_str()))
        .filter(|n| n != "sqlite_offset" && n != "json_each" && n != "json_tree")
        .collect();

    assert!(
        unexplained.is_empty(),
        "the catalog carries {} name(s) the linked engine does not report and \
         this test cannot account for. Either they are real and belong to a \
         family this list should name, or they are invented and would suppress \
         a genuine typo warning: {unexplained:?}",
        unexplained.len()
    );
}

/// Compiled out in this build; see [`over_listing`].
const MATH: &[&str] = &[
    "acos", "acosh", "asin", "asinh", "atan", "atan2", "atanh", "ceil", "ceiling", "cos", "cosh",
    "degrees", "exp", "floor", "ln", "log", "log10", "log2", "mod", "pi", "pow", "power",
    "radians", "sin", "sinh", "sqrt", "tan", "tanh", "trunc",
];

/// Every function name the linked SQLite reports, lower-cased.
fn engine_function_names() -> Vec<String> {
    let conn = rusqlite::Connection::open_in_memory().expect("an in-memory SQLite");
    let mut stmt = conn
        .prepare("SELECT DISTINCT name FROM pragma_function_list")
        .expect("pragma_function_list — the engine's own catalog");
    let names = stmt
        .query_map([], |r| r.get::<_, String>(0))
        .expect("the function list")
        .map(|r| r.expect("a name").to_ascii_lowercase())
        .collect::<Vec<_>>();
    assert!(
        names.len() > 50,
        "only {} names came back, so this pragma is not answering and both \
         tests here would pass vacuously",
        names.len()
    );
    names
}

fn catalog_names() -> Vec<String> {
    schemaic_core::intel::SQLITE_FUNCTIONS
        .iter()
        .map(|f| f.name.to_ascii_lowercase())
        .collect()
}
