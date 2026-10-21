# `core/pg_builtins.rs`

`pg_builtins.rs` — the **PostgreSQL builtin catalog**, the third entry in
`intel::builtin_catalog` and the one that could not be written by hand, and the reason the typo
checker is on for this engine at all. `PG_FUNCTIONS` is 2,706 entries and the split is the point:
**2,682 are generated** from `pg_catalog` on a real PostgreSQL 16.15 — the query that produced
them is in the module doc — and **24 are written by hand** because PostgreSQL's *grammar*
implements those call forms with no `pg_proc` row to read them from —
`trim`, `coalesce`, `cast`, `greatest`, `least`, `nullif`, `row`, `treat`, `grouping`, the
`current_time`/`localtime` family, the `xml*` constructors and the SQL/JSON constructors, each
verified by executing it on the server rather than recalled from the grammar file.
**That second half was found by a test, not by reasoning**: with the generated list alone wired
in, `intel::tests::corpus_valid_queries_are_clean_against_a_loaded_catalog` failed on
`SELECT TRIM(BOTH ' ' FROM name)` with `` `TRIM` looks like a misspelled function ``, because the
grammar rewrites `trim` to `btrim` and `pg_catalog` has no `trim` row. `substring`, `position`,
`overlay`, `extract`, `normalize`, `xmlexists`, `json_object` and `unnest` are deliberately *not*
in that half: they have real `pg_proc` rows as well as grammar, so the query already found them.
**Nothing is filtered out of the generated half.** Type I/O functions (`int4in`), operator
implementations (`texteq`), index support functions (`btint4cmp`) and aggregate transition
functions (`int8_avg`) are all callable SQL, and the catalog's job is to not squiggle a name the
engine really has; a curated "user-facing" subset would be a hand-written list again, with the
same failure mode and no test able to see it. The cost is named rather than waved away — the
near-miss net is drawn around ten times as many names as MySQL's — and
`intel::tests::an_ordinary_user_function_survives_the_pg_catalog` is what keeps it honest,
holding ten ordinary application names (`calc_total`, `audit_log`, `trim_name`) to no squiggle.
**The other half of that cost is time, and it has a measured answer rather than an argument
now.** Ten times the names is ten times the walk on two per-keystroke paths, and putting 2,706
entries where 309 had been was 424.6 ms of `function_typo_checks` per 120 ms debounce tick on a
300-statement PostGIS-shaped buffer. It is the catalog's size that is paid, not its contents, so
the answer is a rearrangement of it and not a cut: `intel::CatalogIndex`, above.
**The completion popup used to pay that cost too, and `PG_SUGGESTED` is what stopped it.** Once
`rank` drew its function tier from `builtin_catalog`, a PostgreSQL tab was offered all 2,706:
prefix filtering and `rank::MAX_ROWS`' cap of 40 meant the list was never dumped whole, but a
short prefix could surface `int4in` or `btint4cmp` beside the name the user wanted. So this
module holds a **second** list — `PG_SUGGESTED`, **863** of the 2,706, sorted, the subset
autocomplete offers — while `PG_FUNCTIONS` is unchanged and is still what the typo checker
trusts, whole. The two questions want different answers on this engine and nowhere else:
*is this a real function here* needs every name including the internals, because squiggling one
is a false positive under correct SQL, and *which name should the popup put under the caret*
needs something a person would type, because plumbing is noise in a list whose whole value is
being short. MySQL's and SQLite's catalogs never needed the distinction — transcribed from a
manual, each already *is* the shorter list — which is why the per-engine fact lives in
`intel::is_offered_builtin` rather than here.
**The cut is mechanical, and that is the whole of why it is allowed to exist**: a curated subset
would be a hand-written list again, with the failure mode above and no test able to see an
omission. The rule is "this function exists to implement something else", which the server can
answer about itself — a name is dropped when its `oid` appears as an operator's
`oprcode`/`oprrest`/`oprjoin`, an index support function (`pg_amproc`), any of `pg_aggregate`'s
support columns, a type's I/O, typmod, analyze or subscript routine, a `pg_cast` conversion, a
range's canonical/subdiff, a language handler or an access-method handler — or when it takes or
returns `internal`/`cstring`, or returns `trigger`, `event_trigger` or one of the handler
pseudo-types. That is **839** names. **The 24
`GRAMMAR_ONLY` call forms are unioned back in by hand, and necessarily**: they have no `pg_proc`
row, so no query over it can return them, and a server-only subset would have dropped
`coalesce`, `cast`, `trim`, `greatest` and `least` — the five most typed names in the list —
while looking like a tidy-up. 839 + 24 = 863.
**`is_suggested` is a binary search, so the sort is load-bearing in the quiet direction**: an
unsorted list answers `false` for names that *are* in it, which shows up as a function missing
from the popup and nowhere else, and `the_subset_is_sorted_and_has_no_duplicates` asserts strict
ascent rather than trusting the generator. The popup does not reach the search per candidate any
more — `intel::offered_builtins` holds the 863 names it answers `true` for, built once per
dialect — so what the sort protects is that one build rather than every keystroke.
In front of the search sits a `LONGEST_SUGGESTED`
(52) length fast-reject, a hand-written number that goes wrong silently in the same direction,
so `the_length_shortcut_rejects_nothing_real` asserts it against the real longest name and then
offers every name in the list back; case-folding is at the caller's end
(`matching_ignores_case_at_the_callers_end`). `every_suggested_name_is_in_the_catalog` keeps the
subset a subset — an offered name the catalog lacks would be a row the checker then squiggles —
and `the_cut_falls_between_plumbing_and_sql` pins both sides of the line, `coalesce`, `cast`,
`trim`, `btrim`, `jsonb_set` in and `int4in`, `btint4cmp`, `texteq`, `int8_avg` out. The live
half is `live::pg_catalog`'s `the_offered_subset_is_still_what_the_filter_answers`, below.
**Names are lower-case**, like `SQLITE_FUNCTIONS` and unlike `FUNCTIONS`, which costs nothing
because both comparisons are case-insensitive at either end — `pg_function_catalog_is_sane` pins
it, along with a size floor set well under 2,682 so a later major version may add or drop names
without a failure that says nothing. **The signature is one overload, not all of them**: `abs`
alone has six and the completion popup has one line, so the representative is the fewest-argument
one, spelled with types rather than parameter names because that is what PostgreSQL's own
documentation prints for an overload set; summaries are the server's `obj_description`, verbatim
but for the leading capital. `intel::f` is `pub(crate)` for this module alone, so 2,682 generated
entries need no second spelling of the `SqlFunction` literal. What holds the file to a real server
is `live::pg_catalog`, under `tests/live/` below.
