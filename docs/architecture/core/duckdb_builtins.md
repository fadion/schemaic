# `core/duckdb_builtins.rs`

`duckdb_builtins.rs` — the **DuckDB builtin catalog**, the fifth entry in
`intel::builtin_catalog`, and with it the reserved-word list `intel` reads for DuckDB tabs
(`DUCKDB_RESERVED`, 75 words — `duckdb_keywords()`' `reserved` category, upper-cased).
**Beside it, `DUCKDB_TYPE_FUNCTION` is the quoter's list, not the checker's**: the 30 words of
the `type_function` category that are not also `column_name` ones — PostgreSQL's join family,
`LIKE`, `IS`, and DuckDB's own `ANTI`, `SEMI`, `ASOF` and `POSITIONAL`. None is reserved, so the
alias rules and the typo checker leave them be, but on 2.0.0 each was a parser error as a bare
table name and as a bare column, and an alias only after `AS`. `must_quote_ident` knew only
`DUCKDB_RESERVED` until a GUI check opened `semi.csv`: `CREATE VIEW` quoted the name, the tab's
`data_file::select_sql` went through `export::ident_if_needed` and wrote `SELECT * FROM semi`,
and DuckDB refused it. The list is `intel::alias_ok_but_unquotable`'s DuckDB arm. The six words
in both categories (`MAP`, `STRUCT`, `COLUMNS`, `GENERATED`, `TRY_CAST`, `TUPLE`) bind as names
and are deliberately left out — subtracting `column_name` is what keeps them unquoted.
`DUCKDB_FUNCTIONS` is 755 entries, **generated, not remembered**, as PostgreSQL's are — DuckDB's
function set is large and every release adds to it — but unlike PostgreSQL's **the generator
needs no server**: the engine is compiled into `schemaic-db`, its version pinned exactly in the
workspace `Cargo.toml`, so the catalog is the linked engine's and nothing else's. The query is
in the module doc: one row per name from `duckdb_functions()`, `internal` functions of `main`
and `pg_catalog`, the overload with the fewest parameters as the signature, then PostgreSQL's
two rules — a signature past 72 bytes is written `name(…)`, an empty description is the literal
``No description in `duckdb_functions()` ``. **Four entries are added by hand** —
`current_time`, `current_timestamp`, `localtime` and `localtimestamp` — because DuckDB's parser
reads them as keywords and rewrites them into a call, so the function list never names them, and
they are what `intel`'s bare-called builtins (`niladic_builtins`) offer.
**Generated under the app's own configuration, and that is load-bearing.** A default DuckDB loads
an extension the machine has installed the first time a query needs one, and a catalog generated
that way held **291 functions no Schemaic session can call** — ICU's among them — since
`db::duckdb` opens every file with extension auto-load and auto-install off. Regenerate it any
other way and the checker trusts names the engine will refuse.
**What checks it is the engine itself, in the other crate's suite**: `db::duckdb`'s
`the_builtin_catalogue_is_the_linked_engines` runs the generator's query on an in-memory
instance opened as the app opens one, applies the same two rules and the four additions, and
compares name, signature and summary entry for entry, and `DUCKDB_RESERVED` and
`DUCKDB_TYPE_FUNCTION` with `duckdb_keywords()` — so a version bump that changes any of them
fails a test rather than leaving the editor squiggling a function the engine has.
`a_type_function_keyword_is_quoted_and_then_names_a_view` checks the other end of the quoter's
list: each word is refused as a bare view name, and `ident_if_needed`'s spelling binds. Both directions, unlike SQL Server's
parser-only oracle. **Names are lower-case**, like `PG_FUNCTIONS`; `is_offered_builtin`'s DuckDB
arm is `true` outright, the function list carrying no plumbing worth cutting from the popup.
