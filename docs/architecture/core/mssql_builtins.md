# `core/mssql_builtins.rs`

`mssql_builtins.rs` — the **SQL Server builtin catalog**, the fourth entry in
`intel::builtin_catalog`, and the one that could be neither generated nor diffed against the
server. Wiring it in is what turned completion's builtins, signature help and the
misspelled-function checker on for SQL Server tabs, all three of which had said nothing on the
`None` arm until then. `MSSQL_FUNCTIONS` is 292 entries, **written by hand** from Microsoft's
*Functions (Transact-SQL)* reference and grouped by its categories — aggregate, ranking and
analytic, conversion, date and time, logical, mathematical, bit manipulation, string, JSON,
system, metadata, security, cryptographic, text and image, rowset. By hand because there is
nothing to generate it from: PostgreSQL's comes out of `pg_catalog`, but T-SQL's intrinsics —
`LEN`, `DATEADD`, `ISNULL` — are in no catalog view at all, and `sys.all_objects` holds the
`sys.fn_*` system functions rather than these.
**What checks it is the parser.** A name T-SQL does not know fails to compile with Msg 195, *is
not a recognized built-in function name*; a builtin called with the wrong arguments fails some
other way; and nothing runs either time. So `live::mssql`'s
`every_catalogued_builtin_is_one_the_server_knows` calls every entry with no arguments — in
`FROM` for the rowset functions, bare for the niladic ones such as `CURRENT_TIMESTAMP` (an entry
whose signature has no `(`) — and fails on a 195, or, for a rowset function, on the Msg 208
*invalid object name* that `FROM` answers for a name it does not know. That second verdict is
new, and it was a hole: a made-up rowset name in `FROM` is a 208 and never a 195, so until it
landed a bogus rowset entry passed. `PREDICT` alone is judged by 195 only, since an empty
`PREDICT()` reads as a table on both versions (and with a `MODEL =` reports Msg 39051, a corrupt
model). It first asserts that a made-up name does come back 195 bare and 208 in `FROM`, so the
test can see a failure on either path at all. **The check is
one-directional, and there is no oracle for the other direction**: it catches over-listing, the
mistake a list written from memory makes, while a builtin the list lacks goes unseen and costs a
squiggle only where the missing name is a near miss of one held — the partial-catalog failure
described under `intel.rs` above, which nothing here rules out. Green on SQL Server 2022 and on
2025 (17.0.5005.3, RTM-CU9).
**SQL Server 2022's set, then 2025's.** The catalog ends with a *New in SQL Server 2025* block of
25 names — `REGEXP_*` (two of them rowset), the fuzzy matchers `EDIT_DISTANCE`,
`EDIT_DISTANCE_SIMILARITY` and the `JARO_WINKLER_*` pair, `UNISTR`, `PRODUCT`, `CURRENT_DATE`,
`BASE64_ENCODE`/`_DECODE`, `JSON_ARRAYAGG`/`JSON_OBJECTAGG`/`JSON_CONTAINS`, the `VECTOR_*`
family and `AI_GENERATE_EMBEDDINGS`/`AI_GENERATE_CHUNKS`. It was the 2022 set until the tier had
a 2025 server to check these against, and they were held back rather than listed unchecked.
**One block at the end, not filed under their categories, because that is the version line** —
and it is load-bearing: `live::mssql`'s `NEWER_THAN_2022` names the same 25, and the oracle
reads the server's `SERVERPROPERTY('ProductMajorVersion')`. Below 17 it excuses the block *and
holds it to the opposite answer* — each name must come back unknown (195 bare, 208 or 195 in
`FROM`, 156 or 207 for a niladic one, `CURRENT_DATE` being a keyword on 2022, Msg 156) — so the
excuse cannot hide a name 2022 in fact has; on 17 it turns `PREVIEW_FEATURES` on in the scratch
database first and holds the block to the parser like any other entry. The server-less
`the_2025_excuse_is_the_catalogs_last_block` pins `NEWER_THAN_2022` to the catalog's tail name
for name, so a 2025 name filed anywhere else, or added without the excuse, fails without a
server. **Five are preview features** — the four fuzzy matchers and `AI_GENERATE_CHUNKS` parse
only in a database with `PREVIEW_FEATURES` on — and are listed anyway, since leaving them out
squiggles correct SQL for the user who has turned it on. `VECTOR_SEARCH` is documented by
Microsoft but is not in 17.0 CU9's parser even then, so it is not listed. **The trade is the
cheap direction**: a tab on 2022 is offered these 25 and cannot call them, since nothing tells
the catalog which version is in front of the tab — the same trade `intel::FUNCTIONS`'s MariaDB 11
block makes against a 10.11 server (`live::mariadb_catalog`'s `NEWER_THAN_BASELINE`). The
checker, trusting the catalog whole, likewise stays silent on 2022 over a 2025 name it cannot
run; the server's own error is what the user sees there. Rowset functions (`OPENJSON`,
`STRING_SPLIT`, `GENERATE_SERIES`, `OPENROWSET` …) are listed too — they are called, in `FROM` —
and the oracle calls them there.
**Names are upper-case**, like MySQL's `FUNCTIONS` and unlike `SQLITE_FUNCTIONS` and
`PG_FUNCTIONS`, which costs nothing because `intel::CatalogIndex` lower-cases every catalog for
its lookups. Nothing is cut for the popup: `is_offered_builtin`'s SQL Server arm is `true`
outright, as SQLite's is, since a list transcribed from a reference carries no plumbing.
`intel::tests::mssql_function_catalog_is_sane` holds it to unique upper-case identifiers, a
signature that names its function, a non-empty summary and a floor of 200 entries;
`sql_server_measures_against_its_own_catalog` puts ordinary T-SQL — `ISNULL`, `DATEADD`,
`EOMONTH`, `IIF`, `TRY_CONVERT`, `SCOPE_IDENTITY` … — through the checker with no "misspelled
function", several of them near misses of MySQL names (`DATEADD` of `DATE_ADD`), and catches
`DATEADDD`; it was watched fail against the `None` arm.
`only_the_engines_with_a_catalog_are_authoritative` covers this catalog too, in both directions — SQL Server knows `datediff` and `iif` and not
MySQL's `curdate`, and neither MySQL nor PostgreSQL knows `eomonth`.
