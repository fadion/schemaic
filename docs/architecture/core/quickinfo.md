# `core/quickinfo.rs`

`quickinfo.rs` — **what a name in the editor names**: the one answer behind the hover's
quick-info and Ctrl+click's go to definition (both under `editor_pane.rs`), except that a name
the statement defines itself is `intel::definition_at`'s, asked first. `name_at` takes the
statement round the offset (`sql::statement_range`), refuses inside a string or comment
(`intel::caret_in_literal_or_comment`), finds the word token the offset is on — or just after
it, since a click at a word's end leaves the caret there — and takes the whole dotted chain
around it, resolving the chain only *up to the part under the offset*, so `o` of `o.total`
names the table and `total` the column. The tokens are `intel::tokenize_range`'s (made
`pub(crate)` for this, with `Token`'s fields and `TkKind`) and the scope is
`intel::scope_levels` — `statement_scope`'s tables kept **one level per query around the
offset, innermost first** — so there is no scanner here of its own. **Names bind as the server
binds them**: a qualifier to the innermost query defining it, a bare column to the innermost
query holding it; one flat list, outermost first, read a subquery's re-used `t` as the outer
table and called a correlated query's bare `id` ambiguous
(`a_name_binds_to_the_innermost_query_that_defines_it`, S8.2-L1-06, red before). A word where a
table reference is written (`intel::on_table_reference`, the table-list walk's own positions) is
a table; anywhere else a bare word is a scope alias, then a column, then a scope table or a
table of the active database — a column first, since a table elsewhere sharing its name took
Sakila's `customer_list.city` (`a_bare_column_named_like_a_table_is_the_column`, S8.2-L1-04, red
before). **A bare column two tables of its query both hold names nothing** — the server refuses
it as ambiguous, and Ctrl+click opening the first table's was a guess; a table joined to itself
is still one (`an_ambiguous_bare_column_names_nothing`). `q.name` is a column of the alias or
table `q`, else a table of the database `q` where one is loaded by that name (MySQL's `db.t`),
else of schema `q` in the active database; three parts are a table's column or SQL Server's
`db.schema.table`; a name followed by `(` is a routine where one is so named and otherwise what
it would be without the paren, so `INSERT INTO orders (id)`'s target is found
(`a_table_name_before_a_paren_is_the_table`, S8.2-L1-05, red before). **Only a table is matched
exact first, then case-insensitive** (`find_table`); a column, a routine, a database and a scope
alias or table are matched case-insensitively from the start, the first listed winning — so of
PostgreSQL's quoted `"Name"` and `name` on one table, whichever is listed first answers for
both, a known gap (R2.1-L8-04). **What no
loaded schema holds is `None`**, never a nearest guess — the database is the semantic authority,
and a guess would open the wrong object. `info` looks a `NamedObject` up as a `QuickInfo` — a
`Table` (or view) with its schema-qualified name, database, column count and comment; a `Column`
as `table.column` with its type, its primary key, nullability, auto increment, default and
generation expression, and its comment; a `Routine` with its arguments and its return type where
it has one — and `QuickInfo::message` is the tip's lines in English: *primary key · not null ·
auto increment · default … · generated as …* under a column's type, a routine as its signature.
`describe` is `info` then `message`, byte for byte what it rendered before the value existed, so
its tests did not move. The value is there because the hover read core's English whatever the
interface's language; the editor words it itself (`quick_info_text`, under `editor_pane.rs`).
**It fetches nothing**: types, constraints and comments are already in the loaded `DbSchema`, so
the lazy fetch once planned for this had nothing to do. The module's own tests pin both halves.
