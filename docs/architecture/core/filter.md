# `core/filter.rs`

`filter.rs` — the header filter/sort bar: a dialect-aware `sqlparser` **AST rewrite** that
splices a `WHERE`/`ORDER BY` into the `SELECT` that produced the result and hands back SQL to
re-run — so filtering covers the whole table, not the loaded page. `build_query` rewrites only
a structurally simple, join-free, CTE-free single-table `SELECT` and degrades to `Ok(None)`
("not filterable") rather than erroring, because eligibility is the caller's question — a
question it now asks `intel::simple_select_source`, which is also what SQLite's write-back
asks, rather than answering inline.
**`rerun_statement` is what the two affordances that re-run a result on screen ask** — the
capped notice's "read N rows" and the export's `All rows` — and it composes *what* would run
(`compose`, or the base verbatim when the filter bar, the sort and the page are all at rest) with
*whether it may* (`sql::rerunnable_for_export`), so the write guard sees the exact string that
would be dispatched — and both affordances re-run the page on screen, not the first one. It lives here rather than at either call site because a term a caller can
delete is a term the suite cannot hold: the export menu's copy of the guard was deletable with
everything green, and the read-more link had no copy at all (*Architecture invariants*, the write
guard). An empty or whitespace base is `None` too, since `contains_write("")` is `false` and a
bare "is it a write?" gate therefore says yes to nothing at all.
**A statement with a constant row limit pages by it**, and a table tab's always has one.
`GridQuery::page` counts from 0, the base as written, and `is_empty` holds only on page 0 — a
page past the first is a view like a filter or a sort, so nothing that asks `is_empty` mistakes
it for the base. `compose` is the one composition both the filter row and a re-run ask for:
the filter and sort through `build_query` (skipped when both are empty, so an unfiltered base
`build_query` could not rewrite is still itself), then `with_page` past page 0. `page_size`
decides whether there are pages at all — the constant `LIMIT n`, MySQL's `LIMIT o, n`, or SQL
Server's `TOP (n)` / `FETCH … n ROWS` (all read by the private `limit_of`) of a statement
`intel::simple_select_source` accepts, and `None` for a parameter, `PERCENT`, `WITH TIES` or a
join (`a_statement_without_a_constant_limit_does_not_page`). `with_page` moves the offset on by
`page × n` from the statement's **own** offset, not from zero
(`a_page_moves_on_from_the_statements_own_offset`), and **a limit read from a `FETCH FIRST n
ROWS ONLY` is written back as `LIMIT … OFFSET …` with the `FETCH` gone** — PostgreSQL takes
both spellings, and the two together are a syntax error there
(`a_fetch_spelled_limit_pages_without_its_fetch`). SQL Server has no `LIMIT`, so there the
`TOP` comes off and the query takes `OFFSET m ROWS FETCH FIRST n ROWS ONLY` — which the server
refuses without an `ORDER BY`, so a statement with none gets `ORDER BY (SELECT NULL)`, an order
the server picks, since a keyless table has no other (`sql_server_pages_with_offset_fetch`; the
rewritten form was run on SQL Server 2022). Which spelling to write is asked of
`SqlDialect::has_limit_clause`, a capability of its own, exhaustive over the engines.
**A page past the first on a statement
with no limit is `Ok(None)`, never the base**: it refuses rather than running the whole table
(`a_page_composes_with_the_filter_and_sort`). **A sort goes ahead of the base's `ORDER BY`,
not in place of it**: the base's own terms — a table tab's key — stay on after the sort as its
tie-breakers, less any naming a sorted column (SQL Server refuses a column twice in one
`ORDER BY`, Msg 169, and the user's direction is the one that counts). **"Naming" is the
engine's rule, and goes through the projection** (`names_column`): the two names compare by
`intel::same_name`, the grid's column name counting as the server's exact spelling, so SQL
Server's `ORDER BY [ID]` is `id` under its case-insensitive collation and PostgreSQL's bare
`Id` stays beside a sort on `"Id"`; an ordinal is the select-list item at its position (none
past a wildcard), and an alias the column it reads, either way round. Each of `[ID]`, `2` and
`bee` over `b AS bee` was kept as a tie-breaker and the sort refused with Msg 169 (measured on
2022; `a_base_term_naming_the_sorted_column_another_way_is_dropped`). Replacing them left a
sort on a non-unique column with ties the engine orders differently for each bound: page 2 of
`ORDER BY status` repeated 98 of page 1's 100 rows on PostgreSQL 16 and 49 on MariaDB 10.11, and
the rows they displaced were on no page (`a_sorted_page_keeps_the_base_order_as_its_tie_breaker`,
`sorting_by_the_base_order_column_does_not_name_it_twice`; both fixed forms overlap by 0 rows
live, and SQL Server's `OFFSET … FETCH` form lands on exactly its window). `page_caption` is
the pager's words — *Rows
101–200* in exact figures, not `text::human_count`'s, because a range whose two ends round to
the same `1.2k` says nothing about where the page is — and offers a next page only after a full
one, since a short page is the last; a table holding an exact multiple of `n` rows therefore
ends on a *No rows after N* page, a full page being unable to say whether another follows
(`the_pager_names_the_rows_and_ends_after_a_short_page`). It counts from `page_offset`, the
offset the statement itself starts at, since that is where `with_page` moves on from — `LIMIT 10
OFFSET 5` shows *Rows 6–15* (`the_pager_counts_from_the_statements_own_offset`).
**`requery` is the question every filter, sort and page change asks first** — `current` with
the caller's `change` applied, or, over staged work, `Err(HoldsStagedWork)` with the change
never made (a value the interface words, `STAGED_REQUERY_MSG` its English), since the re-run's landing would replace the rows the staged edits are indexed by.
It is answered before the change rather than after it so that a refused filter cannot leave the
bar or a header describing a query that never ran; `page_turns` is the same rule for the
pager's chevrons, which dim rather than refuse (the grid's side of both is under *Data grid*).
`eq_condition` is the right-click "Filter by / Exclude" fragment, its literal written for the
column's type (`export::typed_literal`: a SQL Server `datetime` reads the cell's text by the
login's language otherwise). `table_query` is what opening
a table from the tree generates: it orders by the PK on purpose — no engine promises row
order for a capped page, and PG heap order shifts under an `UPDATE` — while leaving an ordinary
name unquoted, since a quoted identifier blinds the mid-edit tokenizer behind completion. It
names a SQLite table **alone**: a connection *is* one file, so there is no server-level
qualifier to add, and `main.t` would be noise on every generated query and wrong the moment the
statement is copied somewhere the file is attached under another name.
Its row limit is the caller's — `spawn_table_tab` passes `schemaic-app`'s `TABLE_TAB_ROWS`,
100 — and it is the limit the grid's pager turns (`a_table_tabs_statement_pages_by_its_own_limit`
holds that for every engine), which leans on the PK order harder than one capped page did: with
no order to page through, nothing promises page two is disjoint from page one, and a keyless
table on an engine with no implicit key is opened unordered.
What it keys and orders on arrives as a **`BrowseKey`**, and the two keyed variants are separate
because they do different things to the projection: `Columns` is the table's own key, already
returned by the `*`, while `Implicit` is a row identity that is **none of the table's columns**
(`TableInfo::implicit_key`, which only SQLite ever sets) and so has to be *named* —
`SELECT rowid, * FROM t ORDER BY rowid ASC LIMIT n`, the projection that makes such a table
editable at all, since nothing can key a write on a value the result doesn't contain. As two
parameters this was a precedence rule buried in the builder; `BrowseKey::pick` is now the one
place it lives, and it has to agree with `edit::analyze_edit` — projecting a rowid the write
path would then ignore carries a column for nothing, and projecting nothing it needed leaves the
table read-only for nothing. A keyed table's statement is byte-for-byte what it was, and two
tests pin that (`table_query_ignores_an_implicit_key_when_the_table_has_a_real_one`,
`table_query_without_an_implicit_key_is_unchanged`). Only the tree's "open table"
(`spawn_table_tab`, via `table_ddl_and_pk`) passes an implicit key: the MCP `describe_table`
sample and the AI seed sample (`sample_sql`) pass `BrowseKey::pick(pk, None)` deliberately,
because both read a table in order to *describe* it and neither writes back, so a rowid there
would be a column of noise.
Quoting here goes through the same rules as the rest of the app; don't add a fourth, and
`needs_quoting` asks `intel::must_quote_ident` — the identifier question — never
`is_reserved_word`, which answers the alias one and on SQLite is a shorter list. Its bare-name
shape is MySQL's on SQLite and SQL Server too, but for a leading `$`, which MySQL takes bare and
the other two do not: SQLite reads `$price` as a parameter and T-SQL as a `money` literal `$`
aliased `price`, so both quote it
(`a_name_that_starts_with_a_dollar_is_quoted_where_that_is_not_a_name`).
The table name itself is **`qualified_table_name`**, which `table_query` and `skeleton.rs` both
call: whether a dialect needs a qualifier at all is a capability answered once, or a generated
`UPDATE` and the browse `SELECT` above it end up spelling the same table two ways. On SQL
Server it always qualifies, because an unqualified name resolves through the *login's* default
schema, which need not be `dbo` — the bare form would read a different table for a different
user — and the cap is `TOP (n)`, through `sql::limited_select`. **PostgreSQL takes the same arm,
`public` included**: a bare name resolves through `search_path`, whose stock first entry is
`"$user"`, so with a schema named after the login the bare `public` form read the login's
`orders` into a tab whose identity, and every write from its grid, stayed `public.orders`
(live, `public_objects_are_named_in_every_statement_that_runs`).
**`bare_name_reaches` is the weaker question completion asks**: whether a bare name *probably*
reaches a table in a given namespace — PostgreSQL's `public` and SQL Server's `dbo` (or no
namespace) yes, any other no, and MySQL and SQLite always yes, their database being the
namespace (`a_bare_name_reaches_the_default_namespace_only`). *Probably*, because the real
`search_path` and the login's default schema are the session's and nothing here reads them —
which is why a caller may only **add** a qualifier on a no, never strip one on a yes, and why
`qualified_table_name` above does not ask it. The `match` is exhaustive, so a fifth engine has
to say where its bare names go.
**SQL Server's `]` is the one quote `sqlparser` will not escape**, and two rules follow from it.
The sort column goes into the AST **already quoted** by the one quoter and wrapped as an
unquoted `Ident`, which renders verbatim: `Ident::with_quote` took the quote character and
escaped it itself — correctly for `"` and `` ` ``, not at all for `[` — so a column called
`x]; DROP TABLE t; --` came back as SQL (`a_sort_column_holding_a_bracket_cannot_end_its_name`).
And a base query or filter that spells a doubled `]]` inside a name (`spells_a_doubled_bracket`)
is not rewritten at all — the base degrades to "not filterable", the filter is refused — since
the round trip returns `[a]]b]` as `[a]b]`, a different statement
(`a_sql_server_query_naming_a_bracket_is_not_rewritten`).
