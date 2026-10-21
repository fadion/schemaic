# `core/intel.rs`

`intel.rs` — the **SQL intelligence** layer (structure-aware, dialect-pluggable). Parses a
*complete* statement with a real per-dialect AST (`sqlparser`; `SqlDialect` seam — MySQL,
PostgreSQL and SQLite all wired) and answers what a token stream can't: `statement_scope`
(the tables and aliases of every query around the caret and the statement's CTE names,
AST-backed with a `skip_noncode` lexer fallback for mid-edit, and a trigger's `NEW`/`OLD` as
its table — below), `clause_context`/`clause_continuation` (caret context + expected-token model for
completion), and `diagnostics` → `Vec<Diagnostic>` (catalog-aware unknown-table, unknown-column via
the per-scope resolver `colres` — qualified *and* unqualified, in a query, `UPDATE` or `DELETE`
(`write_scope`, below), across subqueries/derived-tables/CTEs
with correlation, either one ending in a "did you mean" when one name is near (`nearest_name`,
below) — reserved-keyword-alias errors, syntax errors on completed statements save
those sqlparser has no grammar for (`parser_lacks_statement` and `grammar_gap_masks`,
below), keyword-typo warnings, and two warnings raised per `SELECT` scope as `colres` walks it,
a derived table's own included (`diag_only_full_group_by_inside_subquery`): the
`only_full_group_by` check (`group_by_check`) on a projected bare column neither grouped nor
aggregated — judged only over a single, fully known base table and a grouping list of bare
columns, skipping wildcards and a table whose name another in scope shares with a different key
(`Catalog::pk_shared`, under `predict` below), and exempting a query grouped by the table's
whole primary key, MySQL's own functional-dependency exemption (`pk_of`). **It is drawn only
where every server of the engine refuses the query** (`GroupByRules`, read once per check
from three capability predicates): `rejects_ungrouped_columns` is true for PostgreSQL and SQL
Server, for a MySQL-family tab only once the server is known to be MySQL's (MariaDB's default
mode takes the column, and a tab whose server is not asked yet may be MariaDB's), and never for
SQLite, whose "bare column" is legal; `group_by_sees_output_aliases` lets `GROUP BY d` group
the column `AS d` names (not SQL Server's, whose `GROUP BY` sees input columns only); and
`unique_key_determines_row` exempts a fully grouped unique key of `NOT NULL` columns
(`Catalog::unique_keys`), which MySQL takes as determining the row and PostgreSQL does not
(`group_by_flags_only_what_every_server_of_the_engine_refuses`,
`group_by_an_output_alias_groups_the_column_it_names`,
`group_by_a_unique_not_null_key_determines_the_row_on_mysql`) — and
`cartesian_check` on a `JOIN`/`INNER JOIN`/`STRAIGHT_JOIN` with no `ON`/`USING`, which runs as a
cross join; an explicit `CROSS JOIN` is meant, and left alone, and so is a comma-join with a
`WHERE` — one with no `WHERE` at all to a plain table (not a table function such as `,
unnest(t.a)`, which is correlated) is flagged too (`a_comma_join_without_a_where_is_flagged`).
Both are a `Severity::Hint`, not a warning: the SQL is legal and sometimes meant, and a
warning's amber squiggle is the one a probable typo wears, so these read as typos when they
wore it (`diag_unconstrained_join_flagged`).
The valid-SQL corpora held the bare comma join clean when it was left alone; their entry now has
a `WHERE`).
**The `lints` module** adds warnings on every clean parse of a unit, whatever the statement:
an `UPDATE`/`DELETE` with no `WHERE`, squiggled on its verb (`update_token`/`delete_token`), and
`= NULL`/`<> NULL`/`!= NULL`, squiggled on the comparison, naming `IS [NOT] NULL`
(`an_update_or_delete_without_where_is_flagged_on_its_verb`,
`a_comparison_with_null_is_flagged`). The first is **narrower than the run guard's
`sql::every_row_reason` on purpose**, since a squiggle is drawn on every keystroke's worth of
text and a confirm only on Run: a write that *joins* (`UPDATE … FROM t JOIN u ON …`, MySQL's
`UPDATE a JOIN b`, `DELETE t FROM t JOIN u ON …`) is filtered by its join, and a bounded one
(`TOP (n)`, which reaches it blanked by `grammar_gap_masks` and is read from the unmasked text
after the verb — past any comment, through `skip_leading_noncode`, since a whitespace-only trim
missed `DELETE /* c */ TOP (n)` (R1-L2-03) — or `LIMIT n`) is a batch — while a `FROM`/`USING` that only lists a table is a
cross join and still flagged. Nor does it run inside a T-SQL routine's body (`range_diagnostics`'
`in_body`, from `Unit::header`), where `DELETE FROM staging` is the point of the routine. The
`NULL` check passes over an `EXEC p @a = NULL` whole, whose named arguments the parser reads
as comparisons; T-SQL's `SELECT @v = NULL` is an aliased item and `SET a = NULL` an
`Assignment`, and neither reaches it. A variable compared with NULL anywhere else is flagged —
`WHERE (@p = NULL OR c = @p)` is the optional filter that never applies, which a blanket
exemption for an `@`-name on the left hid while its test passed without it (S8.2-L1-08). One lint is an
**error**, being refused by every engine: an `INSERT` naming its columns with a `VALUES` row of
another length, squiggled on the row's own parens (`Parens`' tokens)
(`an_insert_row_of_the_wrong_length_is_an_error`). **With no column list the count is the
table's**, so `lints::check` takes the `Catalog` and the dialect, and `bare_insert_row_lengths`
measures each row against the table — `` `t` has N columns but this row has M values ``. The
engines do not agree on that count. A row longer than the table is refused everywhere; a
shorter one PostgreSQL fills with the remaining columns' defaults, and the others differ over
the columns the server assigns — MySQL wants every column, `AUTO_INCREMENT` and generated ones
included, SQL Server leaves out an identity or computed one, SQLite **only** a generated one —
its rowid key (`INTEGER PRIMARY KEY`, `auto_increment` in the catalogue) generates a value when
omitted but is still counted. So a row is short, on every engine but PostgreSQL, only when it
is short of `Catalog::bare_insert_columns_of`: on SQLite the columns less those that refuse a
value (`Catalog::refuses_value`, from `ColumnInfo::is_server_assigned`), elsewhere the columns
*no* engine leaves out (`Catalog::insert_columns_of`) — exact for a table with no
server-assigned column, and silent where MySQL disagrees: a MySQL row that leaves out its
`AUTO_INCREMENT` key is refused there and passes here
(`a_bare_insert_row_is_measured_against_the_table`,
`a_bare_insert_may_leave_out_what_the_server_assigns`,
`a_sqlite_bare_insert_leaves_out_only_a_generated_column`). A MySQL/MariaDB `INVISIBLE` column is
out of the count altogether — the implicit column list leaves it out, so counted, the correct
row was the short one and a row naming it the whole one (`Catalog::column_invisible`, keyed as
`insert_skips` is; `a_bare_insert_row_does_not_count_an_invisible_column`, S8.2-L1-01, red
before). `skeleton_columns` still names it, an explicit list being free to. A table the catalogue does not know, a
name two tables share (`Catalog::is_union`) and a three-part name, which may be another
database's, are not measured — `local_table` is that last refusal, shared with the next lint.
**`NOT IN` over a column that can hold NULL is a warning** (`not_in_nullable`): once the
subquery returns one NULL, `x NOT IN (…)` is never true, and the query quietly returns no rows
rather than failing. `Catalog` keeps which columns can (`nullable`, from `ColumnInfo::nullable`,
keyed as `insert_skips` is), and the squiggle sits on the subquery's column, naming both
`WHERE c IS NOT NULL` and `NOT EXISTS`. Only the plain shape is judged — `SELECT c` or
`SELECT t.c`, aliased or not, from exactly one table with no join (`sole_column`) — and a
subquery whose `WHERE` says `c IS NOT NULL` is left alone (`filters_nulls`)
(`not_in_over_a_nullable_column_is_flagged`). That test is *anywhere* in the `WHERE`, under an
`OR` too and with any qualifier, which errs towards silence. So is one that compares `c` in a
top-level `AND` conjunct — `=`, `<>`, `<`, `>`, `IN (…)`, `LIKE`, `BETWEEN` — which is unknown
for a NULL and keeps no such row: the warning told the user to add a filter the query already
had (S8.2-L1-03, red before). Under an `OR` a comparison filters nothing and is not counted. `IN`, an expression, a set
operation and a join are not judged, nor is a column `Catalog::column_nullable` cannot answer:
a table the catalogue does not know, a name two tables share (`is_union`) and a three-part name.
Nor is a bare name the statement declares as a CTE (`ast_scope::collect_statement`'s list, the
one the table check reads): the catalogue's table of that name is not what the subquery reads.
**Inlay hints name the column each `INSERT … VALUES` item goes into** — `insert_value_hints`
→ `Vec<InlayHint { at, label }>`, a label the editor draws in front of byte `at` as phantom text
(under `inline_diff.rs`), so nothing a hint says can be copied, run or saved. Per
`sql::statement_ranges` range it asks the cheap `starts_an_insert` first — `INSERT` or MySQL's
`REPLACE`, past any comment (`sql::skip_comment`, `pub(crate)` for it) — then parses, and labels
only a lone `Statement::Insert` whose source is `VALUES`. With a column list the labels are the
list, and a row of another length is labelled nothing. **With none they are the catalogue's, and
which columns a row names is the engine's question**, asked more exactly than
`bare_insert_row_lengths` above asks it, since a label names a column where a length check only
counts: the visible columns (`columns_of` less `column_invisible`) for a row as long as all of
them **where the implicit list is every column**; the visible `bare_insert_columns_of` for a
row as long as those **where `lints::implicit_list_skips`** answers which — SQL Server, which
leaves an identity or computed column out of the implicit list (`ImplicitSkip::Assigned`), and
SQLite only a generated one (`ImplicitSkip::Refused`), never its rowid key — and there a row as
long as the table is labelled nothing and flagged, *`events` takes 2 values without a column
list but this row has 3*: SQL Server refuses a value for an identity (Msg 8101) or a computed
column (Msg 213), and SQLite counts none for a generated one, all checked live
(`a_full_row_over_a_column_the_server_fills_is_flagged`); the first m where
`lints::short_row_takes_defaults` (PostgreSQL, whose list is the table's in order) for any
shorter row; and nothing for any other length — MySQL lists every column, `AUTO_INCREMENT`
included, and refuses a short row. So `INSERT INTO events VALUES ('a', 2)` over a
server-assigned `id` is `Name`, `at` on SQL Server, `id`, `Name` on PostgreSQL (where the server
does put `'a'` in `id`) and unlabelled on MySQL and on SQLite, which refuses it — *table events
has 3 columns but 2 values were supplied*; labelling it there was the guess this rules out
(`a_bare_insert_takes_its_labels_from_the_catalogue`). A hint on a row the engine refuses would
be a guess. A table the catalogue does not know, a name two tables share and a three-part name
(`lints::local_table`, `pub(super)` for it) get none.
**Where an item starts is lexical, not the AST's**: `row_item_starts` walks between the row's
paren spans (`colres::to_range`) over `skip_comment` and `skip_noncode`, counting bracket depth,
and takes the first code byte after the `(` or a top-level `,` — the expression's own span for
`-1` starts at the `1`. A row whose walk counts other than its AST items is labelled nothing.
Two bounds (`the_hint_pass_is_bounded`): `INSERT_HINT_STATEMENT_CAP` (64 KiB), past which a
statement is not parsed, since a dump's extended `INSERT` runs to megabytes and labelling it
would cost a parse of the whole of it every pass for a hint nobody reads in a dump; and
`INSERT_HINT_CAP`, 2,000 hints a pass whatever the document. `shift_inlay_hints(hints, start,
end, inserted)` carries a set through one edit until the next pass replaces it: a hint before the
edit stays, one at or after its end moves with its text, and one inside the replaced text has lost
its value and goes — so a hint *at* a pure insertion point follows the value the insertion pushed
right (`a_hint_follows_its_value_through_an_edit`).
**AST for classification, `skip_noncode` for byte positions by default** —
except `colres` and `lints`, which use sqlparser 0.62's now-accurate per-identifier *spans* (verified) so the
same column name in an inner vs outer scope is placed independently. **A base table exposes more than
its introspected columns**, so both column checks add `SqlDialect::implicit_columns` — SQLite's
`rowid`/`_rowid_`/`oid`, PostgreSQL's `ctid`/`tableoid`/`xmin`/`xmax`/`cmin`/`cmax`, none on MySQL — to
each base-table source (not to a derived table or CTE, which expose only what they project). Schemaic
*writes* the SQLite one itself: a keyless table's browse statement is `SELECT rowid, * FROM notes ORDER
BY rowid ASC`, and the editor squiggled `rowid` twice, calling its own generated SQL broken. They join
the known set rather than short-circuiting the check, so `rowid` over two tables is still the ambiguity
SQLite itself reports; and the list is per dialect, so `rowid` on MySQL stays the error it is. Two
accepted imprecisions, both erring the way this module always errs (a false "unknown column" is worse
than a missed one): a `WITHOUT ROWID` table genuinely has no `rowid` and the model doesn't record which
tables those are, and PostgreSQL's `oid` is deliberately absent from its list since it belongs to the
system catalogues only. `Catalog` is the case-folded view over
the introspected `DbSchema`s (columns + FK edges); building one is O(tables × columns) and a keystroke asks
for it up to four times, so the UI goes through `CatalogCache` — a memo keyed on the **`Arc<DbSchema>`
identity** of the schemas it was built from rather than a hand-bumped generation, so a re-introspection
misses by construction and there is no write site to remember. Hosts the shared `SQL_KEYWORDS`/`FUNCTIONS`/
`STMT_KEYWORDS` (the UI's completion + editor build on these); shared across engines, so
completion offers a keyword only where `keyword_offered` says the engine takes it — the words
each lacks are `keywords_not_taken`, an exhaustive `match` (under `rank.rs`). **Past those
shared lists, `engine_keywords(dialect, flavour)` is what each engine has of its own** — the window
clauses and `INTERSECT`/`EXCEPT` on all four, then `TOP`, `APPLY` and `OUTPUT` on SQL Server,
`MERGE` there and on PostgreSQL, `RETURNING` on PostgreSQL, SQLite and MariaDB, and `LATERAL`
on PostgreSQL and MySQL. A MySQL-family server not yet asked (`ServerFlavour::Unknown`) is
offered both, as `offered_builtins` offers every builtin then: a row too many is a word the
server refuses, never a write. **It is deliberately not
folded into `SQL_KEYWORDS`**, because that list also answers `is_sql_keyword` — whether a word
can be a table's implicit alias — and `FROM t top` is `t` aliased `top`; a word added for the
popup would have stopped that resolving (`an_engine_keyword_is_still_an_alias`).
`static_words`, the typo checker's exemption set, chains both flavours' answer, lower-cased, so
nothing completion offers on an engine is squiggled there as a misspelling. Also
`join_condition` (FK-aware
`JOIN … ON` auto-fill), `db_error_diagnostic` (positions a live DB error within the statement),
`parses` (Tier-2 gate), and `select_output_names` (a projection's column names *in order*, or
`None` when the statement alone can't say — what `ddl::pg_replaceable` reads).
**`clause_context` takes the last clause keyword within the caret's paren**, not across the
statement: each `(` opens a scope whose keywords end at its `)`, and a closed one gives the
outer clause back. Read across the statement, the last keyword answered wherever it stood,
outside the caret's paren or inside a call — `INSERT INTO orders (cu` was `INTO`'s table slot,
`EXTRACT(YEAR FROM cr` and `TRIM(BOTH ' ' FROM na` the slot of the `FROM` inside the call,
`REFERENCES t (` that of the `ALTER`/`CREATE TABLE` around it, and `VALUES (` got the mixed
list — so a column list or an expression was offered tables, the diagnostics'
`FROM`-inside-a-call mistake (below) in the surface that inserts the word. A paren whose first
word is `SELECT`, `WITH`, `VALUES` or `TABLE` is a subquery and has clauses of its own; any
other is `Column`, but for two shapes: `Table` where it has just opened after `FROM`/`JOIN`
with no word in it yet — it may hold a table or a `SELECT` — or where its own last keyword is
`JOIN`, a parenthesised join; and `Other` for `TABLE t (`, whose column definitions name
nothing that exists yet (`a_paren_scopes_the_clause_context`). Two keywords name a column
without a clause of their own: MySQL's `ON DUPLICATE KEY UPDATE` is read as `SET`, and inside
an `ALTER TABLE` the word after `DROP`/`ALTER`/`RENAME`/`MODIFY`/`CHANGE COLUMN`, or a bare
`MODIFY`/`CHANGE`, is `Column`, where the `TABLE` before it had left a table slot
(`an_upsert_or_altered_column_is_a_column_context`). **A cast's target is `Type`**, asked
before any of that (`cast_target_at`): the word straight after an `AS` whose innermost open
paren is a `CAST(`/`TRY_CAST(` — an `AS` anywhere else is an alias's — or after `::` where the
engine casts with it (`casts_with_double_colon`, PostgreSQL), read off the bytes since the
tokenizer keeps no operators (`a_cast_target_is_a_type_slot`) — and straight after T-SQL's
`CONVERT(`/`TRY_CONVERT(`, whose first argument is the type (`convert_takes_type_first`; MySQL's
`CONVERT` takes it second). **A column definition's type is `ColumnType`**: in a `CREATE TABLE
t (` paren, an item whose only word so far is a name (`defines_no_column` keeps
`PRIMARY`/`CONSTRAINT`/`KEY`/… out); after `ALTER TABLE … ADD`/`MODIFY [COLUMN] name`; after
PostgreSQL's `ALTER COLUMN c TYPE` (`a_column_definitions_type_is_a_type_slot`). It offers
`column_types` — `ddl::common_types`' base names — because MySQL's column types are not its
`CAST` list: `INT` is a column type its `CAST` refuses. **A value no column reaches is
`Value`**: a `VALUES` tuple (the paren whose outer keyword is `VALUES`; a subquery inside it
is a statement again), and the value a statement-opening `DECLARE` (no clause since) or `SET`
(no clause but itself) gives a variable once `past_assignment` finds an `=` in code or a
`DEFAULT` in the list's current item. `rank` offers it the builtins first and keywords, and no
columns — a statement-opening `SET @x =` had offered every column in the database above the
functions (`a_value_with_no_columns_in_reach_is_a_value_slot`,
`a_value_slot_offers_functions_and_no_columns`). `UPDATE … SET a =` stays `Column`.
**`signature_help` asks what the word before the `(` names before it asks the catalogue** —
the column list above, met again in the signature bar: `INSERT INTO log (` opened `LOG(x)`'s
help over a list of columns nobody was calling. `names_an_object` reads back over an `a.b`
qualification to the word before the name and answers yes after `INTO`, `INSERT` (T-SQL's
`INTO` is optional), `TABLE`, `REFERENCES`, `VIEW`, `INDEX`, `KEY`, `FUNCTION`, `PROCEDURE`,
`TYPE`, `WITH` (a CTE's column list) or `EXISTS` (`CREATE TABLE IF NOT EXISTS log (`), and
after `ON` only in a statement that begins `CREATE`
with `INDEX` among its next three words — elsewhere `ON` opens a condition, which may call one.
So `JOIN b ON LOG(` and `VALUES (LOG(` still open it
(`signature_help_does_not_open_on_a_table_named_like_a_function`). `FROM` and `JOIN` are not
on the list, so a rowset function after one — PostgreSQL's `generate_series(` — still does.
**The database's own routines are helped as the builtins are**: `signature_help` takes
`SchemaIndex::routines` (the caller reads the memoised index on each caret move) and a name
one of them has is helped as `name(arguments) → returns` from the server's own rendering of
the list, `Stored function` or `Stored procedure` its summary. One named like a builtin
answers only a **qualified** call (`shop.length(`): MySQL resolves an unqualified native
function's name to the native one (warning 1585 when such a routine is created says so), and
PostgreSQL's search path has `pg_catalog` first, so an unqualified `length(` reaches the
builtin. It was asked first, on the reasoning that the database's own is the one the call
reaches, which is the inverse of both engines' rule. `SignatureHelp`'s text is owned for the
routines' sake; the builtins' is copied out of the catalogue
(`a_stored_routines_signature_is_helped_as_a_builtins_is`).
**`caret_in_literal_or_comment` is the completion popup's gate, and it is not
`caret_touches_noncode`** (under `predict`, below), which differs on both counts that matter
to a list: a quoted name is a name, and completes — `"Ord`, `` `ord ``, `[Order De` are what
opening a table from the tree writes — so only `NonCode::Literal` and `Comment` answer `true`,
through `skip_noncode` and `sql::noncode_kind`. And it asks of `sql[lo..caret]` **with one `x`
appended**, which closes no quote and no comment, so only a span still running past it was open
at the caret: without it the two cases the popup meets most look alike, a `'jo` being typed at
the buffer's end and a `'jo'` just closed both ending where the text does
(`the_caret_in_a_literal_or_comment_is_told_apart_from_a_quoted_name`). The popup's half is
under `completion.rs`.
**`enum_value_slot` is the one literal the gate lets back in** — a `'…'` a column is compared
with, where completion offers the column's enum members (`rank::enum_value_suggestions`). It
is the same `caret_noncode` probe answering `Literal`, with the literal's start found through
`skip_noncode`, and it is narrow on purpose: the left side must be a bare or qualified column
name ending exactly where an `=`, `<>` or `!=` begins (not `<=`, `>=`, `:=`, `==`), or an
`IN (`/`NOT IN (` list with commas and earlier literals between. `(a) = '`, `f(a) IN ('`,
`LIKE '` and a `VALUES (` tuple are not slots — a member is not what is wanted there — nor is a
number. **The `@` check reads the byte, not the token**: the tokenizer drops the sigil, so
`@v = '` would otherwise read as a column `v`. `EnumSlot::value` is the literal's contents —
through the closing quote when it has one, else through the caret — and `closed` says which
(`a_string_compared_to_a_column_is_an_enum_slot`, `other_strings_are_no_enum_slot`). **Only a
quote on the caret's own line closes it**: `skip_noncode` runs an unclosed `'` to the next
quote anywhere below, usually another statement's, and an accept replaced every line up to it
(`a_quote_on_a_later_line_does_not_close_an_enum_slot`).
**`error_fix_range` decides what an AI fix is allowed to rewrite**, and it is a range question
rather than a prompt one: a fix replaces text in the editor, so hand the model the buffer and
every statement the user did not ask about comes back rewritten — which is what the error bar
did, passing `0..sql.len()` unconditionally. One statement in the buffer keeps the whole buffer
(the buffer *is* the statement, comments around it included); with several, the same
`locate_db_error` that places the squiggle finds the offending token and the fix scopes to the
statement holding it; an error naming nothing findable keeps the whole buffer, because guessing
a statement would be a rewrite of the wrong one. **The token has to be unique in the buffer**,
and that is the one place this parts company with the squiggle's lookup: `locate_db_error`
answers with the *first* hit — in code and on word boundaries (`code_word_hits`), so a named
`id` no longer lands inside `valid`, nor a `salery` inside the comment above the real one
(`db_error_locates_the_named_token_in_code_and_whole`) — which is right when it is handed one statement
and wrong when it is handed a script — `near 'FROM t2'` names a word every statement has, so the
fix scoped to the first one, a working statement that Accept would then overwrite while the
broken one stayed broken. A token that appears twice identifies nothing, so `error_fix_range` —
which takes every one of `code_word_hits` and scopes only on exactly one — sends it back to the
whole-buffer fallback. What the text alone cannot settle is a token the failing
statement does not hold at all — a `NOT NULL` column an `INSERT` omitted — which is unique code
in the statement *above*, so `error_fix_range` alone scopes the fix there; that range has to
come from the run. **`run_fix_range` takes it**: the editor's run-error fix hands it what ran
(`run_spans`) — one statement ran, it; several, the one holding the token, else all that ran,
never the lines that did not — and only with no record, or one an edit has cleared, does the
guess decide (`a_run_errors_fix_is_scoped_to_what_ran`, which fails on the guess). In a batch
the pane hands it only the failing panel's own statement (`ui::editor_pane`'s `launch_spans`),
and nothing for a result its last launch did not produce, which sends it to the guess.
**`run_error_diagnostic` is where a failed run is squiggled, and it is handed what ran** — the
buffer ranges the launch sent (`sql::executable_spans`/`selection_spans`), which is the range
`error_fix_range` lacks. One range is the statement that failed whatever the message names, so
it goes to `db_error_diagnostic` (the token, else the first word); with several, the token is
looked for only in what ran, and exactly one holding it decides — two, or none, and nothing is
drawn, since the message does not say which failed. An empty list, or a range the buffer no
longer has, draws nothing either (`a_failed_run_is_underlined_where_the_server_points`,
`a_failed_run_is_underlined_only_in_what_ran`). So Ctrl+Enter on one statement can no longer
land in its neighbour, and Run Everything no longer does either: handed every statement the
batch sent, the several-range search put the squiggle on a healthy statement naming the same
column, and with two holders or none drew nothing — but a batch makes one panel per statement,
so the pane now hands this function the failing panel's span alone (`launch_spans`, under
`ui::editor_pane`). The several-range arm is left to the one launch that sends several ranges
as *one* text — the whole buffer — where the result really does not say which statement
failed. **Where the server placed the error, that decides first**:
PostgreSQL's `P` field arrives as `DbError::QueryAt` (produced by `pg::run_err` on the run path
alone, and only for an `ErrorPosition::Original` — an `Internal` one indexes a function body's
own query), the app carries it on `ModalError::at`, and the editor pairs it with `run_sent`,
where the **one** text it sent began (none for a batch, so a batch statement is placed by the
token search over its one span, never by the server's position). It is a 1-based *character* index — measured on PostgreSQL 16, `nope`
after an `é` is character 13 and byte 14 — trusted only inside what ran, and it picks the
second of two `x`s where the token search takes the first
(`a_servers_own_position_places_the_squiggle`). On a quoted name it points at the opening
quote, so the squiggle is the span `skip_noncode` gives the name, quotes and all — the offline
check's range — where it was the quote alone, which an AI fix counted as a second problem
beside the offline one (`a_servers_position_on_a_quoted_name_takes_the_whole_name`, red
before). The text sent is the editor's with **only the tab's parameters rewritten**:
`run_statement` sends it whole through the simple protocol, and the read-only `SET` and a
Manual tab's `BEGIN` go as their own messages — but `params::prepare_run` has replaced each
`:name` by its value first, so the server counts in that text, and a placeholder is not the
length of its value. Used unmapped, `:name` bound to a 26-letter string moved the squiggle 23
characters on, onto a healthy word. The pane maps it back through `params::template_position`
(`substitute`'s walk, stopped at the position; inside a value it is the placeholder's `:`) with
the tab's bindings, and a parameter changed after the launch clears `run_sent`, so a value the
run did not use never measures it
(`a_servers_error_position_is_mapped_back_through_the_substitution`, red before).
**`code_names` is that same scan reduced to a yes/no, and it is `pub` for a caller outside this
crate.** `db::sqlite`'s introspection needs to know which triggers name a table — that is
`TableInfo::referring_ddl` — and a `contains` there matches the word inside a comment or a `'…'`
default and so refuses an edit for a trigger that does not mention the table at all. It is
`code_word_hits` and therefore the one boundary lexer, which is what keeps a *backend* from
growing a scanner of its own; `ddl::rebuild_strands_a_trigger` is the other caller, asking the
same question of a column name. `problems_in_range` is its companion for the
editor's own diagnostics — every message touching a range, ordered, **deduplicated**, since the
offline and DB-validated passes both report an unknown column and the count reaches the user as
"fix these 2 problems". They never report it in the same words (`Column … not found` against the
server's `Unknown column …`), so a dedup on text never fired: the key is the **token** — two
errors on the identical range are one problem, and the first (the offline one, which the caller
puts first and which carries the "did you mean") speaks for it. The text it compares and
returns is `Diagnostic::message()`, core's English, in every interface language, since the
model reads it. Both of its arms are **half-open**, the zero-width one
included: a point sitting on a statement boundary belongs to the range that starts there, and
read inclusively it was reported for both neighbours at once.
**A keyword's position is not its meaning, and the diagnostics' worst false positives were all
that one mistake.** `table_refs_with_pos` reads every `FROM`/`JOIN`/`INTO`/`UPDATE` as opening a
table list, which is right for a subquery's `FROM` and wrong inside a call: SQL spells four
functions with a `FROM` between their parentheses (`EXTRACT(YEAR FROM order_date)`,
`TRIM(BOTH ' ' FROM note)`, `SUBSTRING(note FROM 1 FOR 3)`/`SUBSTR`,
`OVERLAY(a PLACING b FROM 1 FOR 2)`), and `SELECT EXTRACT(YEAR FROM order_date) FROM orders`
drew a red ``Table `order_date` not found`` on both MySQL and PostgreSQL, on a statement MariaDB
10.11 runs. `SUBSTRING(note FROM 1 FOR 3)` escaped only by luck — its operand is a digit, and a
digit is not `is_word_start`. `from_separates_call_arguments` is an **allowlist of those
functions, not "any call"**, because the tempting rule ("a `(` directly preceded by a word is a
call") sorts a subquery onto the wrong side: `FROM (SELECT …)`'s paren is also preceded by a
word, namely `FROM`. Naming the functions cannot reach a subquery at all, and the standard's list
of them is closed. `enclosing_calls` — for every token, the word before the innermost *unmatched*
`(` — is the shared primitive, computed **once per token list in one forward pass over a stack of
open parentheses**; it was a walk back from each `FROM` asked about, and a top-level `FROM` never
meets the `(` that stops it, so a pass cost O(`FROM`s × tokens) and diagnostics on a
16,000-line T-SQL procedure — one range to the end of its batch — took 1.6 s on the keystroke
path (`diagnostics_work_grows_linearly_with_a_long_range` counts the steps rather than timing
them, since a count fails reliably and a stopwatch does not). `as_introduces_a_type` is its
second caller: `CAST(x AS CHAR)` puts a
**type** after `AS`, so every MySQL cast target that is also a reserved word (`CHAR`, `UNSIGNED`,
`DECIMAL`, `BINARY`, `CHARACTER`) was squiggled as a botched alias; `SIGNED` escaped only by
being absent from `MYSQL_RESERVED` and `CONVERT(a, CHAR)` by not using `AS`, which is what says
the `AS` form is the whole of it. It is answered off the token walk rather than the AST because
`alias_checks` runs unconditionally, parse failure included — gating it on a parse would miss the
real thing the diagnostic is for, since sqlparser *accepts* `AS or`.
**A function after `FROM`/`JOIN` is not a table** either: a rowset builtin read as one — T-SQL's
`STRING_SPLIT`/`OPENJSON`/`GENERATE_SERIES`, PostgreSQL's `generate_series`/`unnest` — was
``Table `…` not found`` (`a_function_in_from_is_not_a_missing_table`). After `INTO` the `(`
opens a column list, and that table is still judged. A table name *is* followed by `(` in one
place: T-SQL's old-style hint, `FROM t (NOLOCK)` — one hint alone, without `WITH`, which 2022
and 2025 still run before or after an alias, and which read as a call left `FROM nosuch
(NOLOCK)` unjudged. `table_hints` is the per-dialect answer (`None` off SQL Server), holding
the hints measured to run alone; two together, a bracketed `[NOLOCK]` or any other word the
server takes for a call's arguments (Msg 207, 215), and `HOLDLOCK`/`INDEX` it refuses without
`WITH` (Msg 1018), so those stay calls; `WITH (…)` ends the item without ending the list
(`a_t_sql_table_hint_without_with_is_not_a_call`). **The list is one reader**,
`read_table_list`, shared by `located_table_refs` and completion's `lexer_scope`, whose two
copies had drifted: it reads every part of a name (`dotted_name`), so `lexer_scope` too sees
`company.dbo.employees e` as `employees` in `company` with its alias, where it saw the table
`dbo` and `e.` completed nothing (`a_three_part_name_is_in_completions_scope_mid_edit`); it ends at a
reserved word only **unquoted**, so `[order]`, `` `group` `` and `"user"` — quoting being the
remedy for exactly those names — are judged (`a_quoted_reserved_table_name_is_checked`); and
**a list goes on past a parenthesised item**: a derived table or a call stopped it on its `(`,
where the comma test found none, so `FROM generate_series(1, 3) g, nosuch` never judged
`nosuch`. The reader stops at the `(` and returns where the list resumes (`ListStop::resume`,
past the `)`); the caller reads the inside first — a derived table has `FROM`s of its own —
and `read_table_list_after_paren` the alias and the rest
(`a_from_list_is_read_past_a_function_or_a_derived_table`). Three more shapes stopped it early,
each hiding a missing table after them. PostgreSQL's `WITH ORDINALITY` stands between a rowset
call's `)` and its alias, and read as the list's end (`a_from_list_is_read_past_with_ordinality`).
`FROM a JOIN b ON …, c` is one list of two items, and the reader ended at the `ON`:
`join_tail_comma` finds that comma at the join's own paren depth, and gives up at a `;`, at a
`)` closing the enclosing paren, or at the first clause word — `GROUP`, `SET`, `LIMIT`,
`WINDOW`, `RETURNING`, `FOR`, `DUPLICATE`, the next `JOIN`, a statement head that needs no `;`
— because stopping early only misses a table, while going on judges `GROUP BY a, b`'s columns
as tables (`a_from_list_is_read_past_a_join`). A `JOIN` that read no item — T-SQL's
`OPTION (HASH JOIN, LOOP JOIN)` — asks nothing. `ListStop::resume` is a `Vec`, innermost last,
holding that comma as well as the `)`s a list goes on past. And `FROM (a JOIN b ON …)` was taken
for a derived table, so `a`, after no `FROM` or `JOIN` of its own, was never read:
`opens_parenthesised_join` decides by the token after the `(` — a further `(`, or any name but
`SELECT`/`WITH`/`VALUES`/`TABLE`, asked of those four words rather than `is_reserved_word`
because `SQLITE_RESERVED` holds only what SQLite refuses as an alias — and the reader reads the
join's first items as a `FROM` list, leaving its `JOIN`s to the caller's walk. A
`((SELECT …) UNION (…))` is asked again at its inner `(` and still ends a derived table; MySQL's
`USE INDEX FOR JOIN (i)` opens no list (`a_parenthesised_join_registers_its_tables`).
**The walk over those lists is one too**, `walk_table_lists`, and `alias_checks` is its second
user and the reader's third: its implicit-alias half (`FROM orders or`) walked `FROM`/`JOIN` on
its own and had drifted the way `lexer_scope` had — it stopped at a derived table's or rowset
call's `(`, at a join's `ON` and at a quoted reserved table name, so a reserved alias after any
of them went unreported (`a_reserved_alias_is_flagged_wherever_the_from_list_goes`). It passes
`targets` false, keeping the check off `INSERT INTO`/`UPDATE`, whose `VALUES`/`SET`/`SELECT`
would false-trigger, and reads `ListItem::reserved_alias`, which the reader fills for a slot
`read_alias` left alone (`reserved_in_alias_slot`) and for an unquoted implicit alias it *took*
that some server of the dialect reserves (`reserved_anywhere`) — `alias_checks` then asks the
server's own flavour whether it is an error (under the reserved lists below). An `AS
<reserved>` is the explicit scan's, which still runs over every token.
**`INTO` names a table only after `INSERT`/`REPLACE`** (`insert_precedes`), because the word has
three other meanings and none of them does: PostgreSQL's legacy `SELECT a INTO newtbl FROM t`
names the table it is about to *create*, and MySQL's `SELECT a INTO @x` and
`INTO OUTFILE`/`DUMPFILE` name no table at all — `@` is not `is_word_start`, so `@x` tokenized as
the bare word `x` and looked exactly like one. Both drew a confident ``Table `x` not found``,
with no hedging available, because the active database *is* loaded and `table_status` therefore
has no `Unknown` arm to fall back on. It asks the previous word *or* the statement's leading
keyword, so `INSERT IGNORE INTO t`, `REPLACE DELAYED INTO t` and PostgreSQL's data-modifying
`WITH … INSERT INTO t` all still register their table.
**`is_implicit_alias` is "is this bare word an alias" written once instead of three times, and
the order of its two tests is the bug it fixes.** `ends_table_ref` first, because
a join or clause keyword *ends* a table reference rather than naming it, and only then
whether the word is reserved — `reserved_on_every_server`, as `read_alias`'s `AS` arm asks too
(under the reserved lists below). Only SQLite showed it: `MYSQL_RESERVED` and `PG_RESERVED` both carry
`LEFT`/`INNER`/`CROSS`/`NATURAL`/`RIGHT` (MySQL has no `FULL` join to reserve), so on those engines the reserved test alone
happened to reject them, while `SQLITE_RESERVED` holds only what SQLite refuses *as an alias* and
that engine accepts all seven as identifiers. So
`SELECT * FROM orders LEFT JOIN customers ON |` registered `orders` under the alias `LEFT`,
`ref_qualifier` preferred the alias, and both FK auto-join surfaces inserted
`"LEFT".customer_id = customers.id` — `no such column: LEFT.customer_id`. A bare `JOIN` was
unaffected, which is why it read as working. `alias_checks` had the right order all along and was
the third caller; spelling it three times is what let two of them drift, and `read_alias` is now
the only one — `alias_checks` reaches the slot through the shared walk. A **quoted** word is
always an alias, so the caller checks `Tk::quoted` before asking.
**`is_table_ref_continuation` grew a `RETURNING` arm for the same reason**, one keyword further
on: it ends the table reference of a data-modifying statement, and without it the alias check
read `RETURNING` as an alias for `zap` in `DELETE FROM zap RETURNING *` — reserved on
PostgreSQL, so the standard archive idiom was squiggled as broken. **Not dialect-gated**, because
MariaDB has `RETURNING` too. **Nor is its `WITH` arm**, which opens a SQL Server table hint
(`FROM t e WITH (NOLOCK)`) — read as the reserved word used as an alias — since no engine here
takes `WITH` as one. The hint's inside needed a second fix in `colres`: sqlparser carries a
hint as an `Expr`, so the visitor resolved `NOLOCK` as a column (``Column `nolock` not
found``); `Collector::not_columns` records each `TableFactor::Table`'s `with_hints` ranges in
`pre_visit_table_factor`, and an expression inside one is skipped. A `FETCH` arm ends
`FROM t FETCH FIRST 5 ROWS ONLY` the same way. The explicit `AS <reserved>` scan has its own
exemptions: a query body after `CREATE … AS` **or `ALTER … VIEW … AS`** (MySQL's and SQL
Server's redefinition), and the `OF` of SQL Server's temporal `FOR SYSTEM_TIME AS OF`
(`a_view_body_a_temporal_clause_and_fetch_are_no_alias`, S8.1-L1-02/-03/-06, red before).
**A select-list alias
resolves an unqualified name except in the clauses the engine reads before it**
(`Scope::alias_blind`): `WHERE` everywhere but SQLite, which resolves one there
(`select_alias_in_where`), a join's `ON` likewise (`select_alias_in_join_on`, its ranges from
`output_alias::join_ons`, which the rename already read), and `HAVING` on PostgreSQL and SQL
Server, which do not (`select_alias_in_having`); the resolver used to know only `WHERE`, for
every engine (`a_select_list_alias_is_judged_by_the_engines_scope`), and then let an alias
answer a name in `ON` everywhere, where MySQL says *Unknown column 'k' in 'on clause'*
(`diag_alias_in_join_on_flagged_where_the_engine_rejects_it`). **On PostgreSQL a bare table name or
alias is the whole row** (`names_whole_rows`): `row_to_json(u)`, `SELECT u FROM users u` —
`resolve` accepts one naming a visible source once no column of that name has claimed it,
where it used to report ``Column `u` not found``
(`a_whole_row_reference_is_not_an_unknown_column_on_postgres`). **A key with a non-ASCII
letter in it is compared fully folded where the engine ignores every letter's case**
(`colres::holds`, over `SqlDialect::names_fold_unicode_case`): the keys are ASCII-folded, so
`ORDER BY été` against `AS Été` was squiggled missing on MySQL and SQL Server, which bind it,
with an Alt+Enter "fix" to the name already there; SQLite and PostgreSQL fold ASCII alone and
keep the flag (`a_non_ascii_case_difference_is_no_missing_column_where_the_engine_ignores_it`).
A derived table's column
list renames its columns as a CTE's does (`cte_cols`): `(SELECT id …) AS d(x)` exposes `x`
and not `id` (`a_derived_tables_column_list_names_its_columns`). **Three more identifiers
the parser hands over are not columns**, all measured as red ``Column `…` not found``: a
datepart function's first argument (`DATEADD(day, …)`, and MySQL's `TIMESTAMPDIFF(SECOND,
…)` — `datepart_functions`, per dialect; the call's span goes into `not_columns` before the
visitor descends), a variable whose identifier keeps its `@` (`WHERE id = @x`, `@@ROWCOUNT` on
SQL Server, `@x`/`@@version` on MySQL), and a builtin called without parentheses
(`SYSTEM_USER` — `niladic_builtins`, read off `builtin_catalog` where an entry's signature is its
bare name, plus `bare_called_builtins`, a per-dialect list of what the engine calls bare and
its catalogue does not spell so: that rule alone found **nothing** on MySQL, whose catalogue
writes `CURRENT_USER()`, or on PostgreSQL, whose generated one has no bare form, so `SELECT
CURRENT_USER` and `current_schema` stayed red — every name on the list was run bare on MariaDB,
MySQL 8 and PostgreSQL 16, and a quoted one is a column again,
`a_bare_called_builtin_is_not_a_column_on_mysql_or_postgres`). The same set decides what
completion inserts: `called_bare` is its `pub(crate)` question for `rank` (under `rank.rs`).
`sql_server_measures_against_its_own_catalog` had filtered its statements to the
misspelled-function message and so passed over ``Column `day` not found`` on the one it named;
it asserts no diagnostic at all now (`a_datepart_variable_or_niladic_builtin_is_not_a_column`). `INTO` (MySQL's trailing
`SELECT … FROM t INTO @x`, a cursor's `FETCH … FROM c INTO @x`), T-SQL's second `FROM`
(`DELETE FROM t FROM t JOIN u …`) and `PIVOT`/`UNPIVOT` end a reference the same way.
**A statement's end ends one too, and the tokens could not see it**: a routine body is one
range holding many statements, the tokenizer dropped `;`, and `employees; SELECT` read exactly
like `employees SELECT` — a reserved-alias error per statement, on MySQL's `DELIMITER` bodies
as on T-SQL's. `Token::after_semicolon` is a flag rather than a token so no consumer that reads
adjacent tokens sees a different stream, and both scanners stop at it. T-SQL needs no `;` at
all, so `ends_table_ref` also asks `unterminated_statement_heads` — the words that begin a
statement where none needs a terminator, an exhaustive per-dialect `match` empty on the three
engines where `FROM t select` is the mistake the check exists for
(`a_statement_end_ends_the_table_reference_before_it`). A cursor is not a table either:
`fetch_precedes` reads `FETCH [NEXT | … | ABSOLUTE n] FROM c` (and PostgreSQL's `MOVE`) as
naming one, walking back over at most a direction and its count so a column named `first`
before a real `FROM` is left alone. The verb is the dialect's (`cursor_verbs`: `MOVE` on
PostgreSQL alone, nothing on SQLite, which has no cursors), never one after a `.` (`e.move` is a
column), and one the dialect does not reserve only where a statement begins — taken on every
engine and anywhere, `SELECT id, move FROM nosuchtable` skipped its table list
(`a_column_named_move_or_fetch_is_not_a_cursor`).
**T-SQL is where sqlparser 0.62's grammar runs out, and a SQL Server hand check found ordinary
T-SQL squiggled red.** `parser_lacks_statement` withholds the parse error — only that; the
typo, function-typo and alias checks still run over the statement — for what that grammar does
not carry: `RECONFIGURE`, `CREATE [UNIQUE] [NON]CLUSTERED INDEX`, a routine's or trigger's
*header* (a scalar function's `RETURNS … AS`, a procedure's bare `@x int` parameters, `WITH
SCHEMABINDING` / `CALLED ON NULL INPUT`, a trigger's `WITH EXECUTE AS`; each ran clean on SQL
Server 2022), and the ordinary statements a body is made of that it lacks — measured by running
the diagnostics over every `sys.all_sql_modules` definition on SQL Server 2022: `SAVE TRAN`,
`BEGIN`/`COMMIT`/`ROLLBACK TRAN name`, `EXECUTE AS` and `REVERT`, `DBCC`, `CHECKPOINT`, `BULK
INSERT`, `BACKUP`/`RESTORE`, `WAITFOR`, `ENABLE`/`DISABLE TRIGGER`, a cursor's `FETCH`, a cursor
variable's `SET @c = CURSOR …`, Service Broker's statements, `CREATE`/`ALTER` of a sequence,
synonym, table type, queue and the like, `CREATE`/`UPDATE STATISTICS`, `INSERT … EXEC`, a
`DELETE t WHERE` with no `FROM`, and the T-SQL forms of `ALTER TABLE` (`ALTER COLUMN`, `ADD …
DEFAULT … FOR`, `[NO]CHECK`, `SWITCH`, …). Checking bodies made the same corpus — `master`'s
`sys.all_sql_modules` and `msdb`'s `sys.sql_modules`, on 2022 and 2025 — red in 80 and 89
modules (`%%` internals aside) until the rest of what they hold was listed too:
`CREATE`/`ALTER`/`DROP` of the object kinds the grammar has no statement for
(`TSQL_KINDS_THE_GRAMMAR_LACKS`: `SYNONYM`, `LOGIN`, `CERTIFICATE`, `EVENT NOTIFICATION`,
`PARTITION …`), T-SQL's users, `ALTER DATABASE`/`AUTHORIZATION`/`INDEX`, `ALTER SCHEMA …
TRANSFER`, `DROP TRIGGER … ON ALL SERVER`, a `GRANT`/`DENY`/`REVOKE` on a class-qualified
securable or with a permission only SQL Server has (`grant_beyond_the_grammar`), `ALTER
TABLE`'s `CHECK CONSTRAINT`, `ADD a, b` and `DEFAULT … WITH VALUES`, and the deprecated
`READTEXT`/`WRITETEXT`/`UPDATETEXT`/`SETUSER`. The kinds the grammar carries are not listed, so
a mistake in a `DROP TABLE` still reports. The server stays the authority for those, as for
everything else. MySQL and SQLite answer `false`; PostgreSQL withholds a `DO` block's alone,
which its grammar lacks outright and whose body is a string to the lexer, so nothing inside it
went unchecked before — a `DO;` with no body still errs (`a_postgres_do_block_draws_no_error`).
**A clause the grammar lacks inside an
ordinary statement is read past rather than given up at**: `grammar_gap_masks` finds T-SQL's
query hint `OPTION (…)` at the top level, `WITH ROLLUP`/`WITH CUBE` after a `GROUP BY` at the
same depth, and `GROUPING SETS (…)` (`MsSqlDialect` leaves `supports_group_by_expr` off, so
that stopped at `SETS`; all run on SQL Server 2022 and 2025) — and, for the statements a body
holds, a compound assignment's operator where an assignment stands (`SET @i += 1`, an
`UPDATE`'s `SET col *= 2`; `compound_assignment_at` — in a `WHERE`, `*=` is the outer-join
operator SQL Server removed and refuses, and blanking it anywhere passed it), `NEXT VALUE FOR
seq`, `EXEC @rc = p` and an `EXEC`'s positional `OUTPUT`/`OUT`, a cursor's options and `FOR
READ ONLY`, `PARSE`/`TRY_PARSE`'s `AS type USING culture` to the call's own `)` (the first `)`
was `decimal(10,2)`'s), `TOP (n)` after `DELETE`/`UPDATE`/`INSERT`, a
table hint on a write's target, `DELETE FROM t FROM …`'s first `FROM` (behind a `WITH`'s CTEs
too), `CREATE INDEX`'s `WITH (…)` and filegroup, a trigger's `UPDATE(col)` (filled as a call),
and what the module corpus held: an index's kind in a table definition (`PRIMARY KEY
CLUSTERED`), a `CREATE TABLE`'s trailing comma (a table variable's and a table type's are
refused, and still err), a column's `FOREIGN KEY REFERENCES` and a column-level key given its
own column list with no comma before it, a join hint after its join type (a bare `HASH JOIN` is
refused), `col.WRITE(…)`, `WHERE CURRENT OF c`, `WITH XMLNAMESPACES`, `OPENXML`'s `WITH`
schema, `AS N'alias'`, space-separated table hints (`table_hint_commas`), `f(@a, DEFAULT)`, a
collation after a qualified call, `< =`, `0X00`, `FROM ::fn_x()`, ODBC's `{fn …}`, a
procedure's group number `p;2` (`numbered_procedures`, which also keeps that `;` from ending
the statement or the range) and the internal `OpenRowset(TABLE x)` the server's own modules
read and refuses anywhere else. With those, and a view read as one statement — its `AS WITH c
AS (…)` was cut from the `SELECT` its CTEs feed, and those CTEs are now its own to the table
check — the corpus draws no error on either version outside the `%%` internals
(`the_constructs_of_sql_servers_own_modules_draw_no_error`,
`t_sql_module_constructs_read_past_hide_nothing_beside_them`) — and `unit_diagnostics`
parses the statement with each overwritten — blanks, or a literal where an expression must
remain — byte for byte (`masked_text`), so every offset and line still lands where it did. A
clause is masked only where it really is one, so the same words anywhere else still err
(`t_sql_clauses_read_past_hide_nothing_beside_them`). The error used to be
*withheld* when it landed on such a clause, and sqlparser stops at its first error, so a real
one after it (`… WITH ROLLUP HAVING COUNT(*) >`) and every check that needs a parse went with
it, while `FROM t WITH CUBE` — Msg 336 on the server — passed because only the next word was
asked (`a_clause_the_parser_lacks_hides_nothing_after_it`,
`t_sql_grouping_extensions_draw_no_syntax_error`). `TABLESAMPLE` also ends a table reference,
and so does `OPTION` with its `(` after it, since the alias check read `FROM t OPTION
(RECOMPILE)` as a reserved alias — a bare `FROM t option` is that mistake still, refused by SQL
Server and MySQL 8 (`a_read_past_rule_covers_only_its_construct`). The function-typo check asks
nothing of a word right after `AS`, a type (`AS numeric(5)`) or an alias's column list (`AS
v(x)`) and never a call (`parse_to_a_parameterised_type_draws_no_error`).
**A range is not always one statement.** T-SQL needs no `;` (`statements_need_no_terminator`,
exhaustive per dialect), so a script in the language's normal style is one range — and the last
range without a `;` is the fragment still being typed, so its parse error was withheld and, a
multi-statement blob never parsing, its table and column checks never ran: no error anywhere in
such a script. `range_diagnostics` cuts each range again into `statement_units` and checks each
alone; only the last unit of the last range is the typing tail. The statement cuts
(`tsql_statement_end`) are deliberately **not** `sql::tsql_statements`', which may cut wrongly
because a fragment there is one no guard answers for — here a wrong cut is a false syntax error
(`INSERT INTO t (a)` cut from its `SELECT`, `ALTER TABLE t` from its `DROP COLUMN`). So it cuts
at a `;`, a label and before a word that can only begin a statement in its context
(`TsqlStatement::cuts_before`: `SELECT` unless it follows a set operator, `AS`, `FOR` or an
`INSERT` still owed its rows; `SET` unless it is an `UPDATE`'s first or follows `ON DELETE`;
`UPDATE`/`DELETE` unless they follow `THEN` or, in an `ALTER`/`CREATE`, the `ON` of a key's
referential action — any other `ON` is a `SET NOCOUNT ON`'s, and exempting it glued the write
after it to the `SET` and cut away the write's own `SET`, two red errors on the first lines of
most procedures (`a_set_option_on_does_not_glue_the_next_write_to_it`); `IF` unless it is a
`DROP TABLE IF EXISTS`; …), never inside a `CASE`, nor in a `GRANT`/`REVOKE`/`DENY` before
its principals (its `TO`, a `REVOKE`'s `FROM`), whose privilege list is made of these words —
after them a statement begins as anywhere else, and without that cut a `GRANT` swallowed the
rest of a body, its `IF … GOTO` parsed again and, unterminated, all of it the typing tail
(`a_grant_ends_after_its_principals`) — and a cut it
misses costs nothing, because `parse_statements` parses a unit without requiring `;` between
statements, which is how the server reads it
(`a_t_sql_script_without_semicolons_is_checked_statement_by_statement`,
`t_sql_statement_units_cut_only_where_a_statement_begins`). **Control-of-flow is read, not
parsed** (`tsql_units`): sqlparser has no `TRY … CATCH`, label, `GOTO`, `BREAK` or `CONTINUE`,
and reads a `BEGIN … END` block or an `IF … ELSE` whole, so one construct it lacked anywhere
inside took the whole block down — and a `;` inside a block, where the editor's statement bounds
cut, left an unclosed `BEGIN`. Each keyword is a `UnitKind::Structure` unit, an `IF`'s or
`WHILE`'s condition (up to the statement it governs) a `UnitKind::Condition` parsed as an
expression, and the statements between are checked one by one, so the result is the same
whether a block reaches the checker as one range or several
(`t_sql_control_of_flow_holds_checked_statements` asserts both). A condition can hold no token
of its own — `IF 1 = 1 BEGIN`, numbers and operators being none — so `tsql_units` marks it
from its keyword's end; it had taken the `BEGIN` for its condition. **Reading it as structure
lost the parser's one check of it, that it pairs up**, and `TsqlFlow` puts it back: a batch's
units, read across its ranges and reset at a `GO`, against a stack of what is open, on T-SQL's
own grammar — an `IF` governs one statement and takes an optional `ELSE` (the nearest open
`IF`'s), a `WHILE` one statement, a label counts as one (`IF @x = 1 lbl: ELSE …` runs), and
`END TRY` is followed at once by `BEGIN CATCH`, not even a `;` between (measured). An `ELSE`
with no `IF`, an `IF` with nothing before its `ELSE`, a stray `END`, an `END TRY` with no
`BEGIN CATCH` are errors where they stand; a block still open at the buffer's end is one unless
that end is the fragment still being typed. It also asks the two `;`s the rest of T-SQL does
without: before a `THROW` the statement must end with one (Msg 102 after a statement, `END`,
`BREAK`, `GOTO` or `END CATCH`; none needed after `BEGIN`, `ELSE`, a label, a condition or a
routine's `AS`), and a `MERGE` must (Msg 10713), which `range_diagnostics` asks of each unit
(`unbalanced_t_sql_control_of_flow_is_reported`,
`throw_and_merge_need_the_semicolon_sql_server_asks_for`). A routine's header, up to its
body's `AS`, is a statement of its own, withheld as above, and **its body is checked like any
script** — it was one unit whose parse error was withheld wholesale, so a body was never
syntax-checked at all. The tokenizer reads a number as no token, so `@x < 0.0 SELECT` is not the
qualified name `@x . SELECT` that hid the next statement's first word. The routine statements broke the alias
check a second way: a T-SQL header *ends* in a mandatory `AS` whose next word is the body's
first statement, so `AS SET`, `AS BEGIN` and `AS RETURN` were reserved-alias errors. `routine_body_as` finds
that `AS` — the first outside parentheses not preceded by `EXECUTE`/`EXEC` nor by a parameter
(`@x AS int`: a procedure's parameters take no parentheses, so the parameter's `AS` won) — and
the check skips it alone, so an alias inside the body is still checked
(`t_sql_routine_exemptions_leave_the_body_checked`, `ordinary_t_sql_draws_no_squiggle`). It is
gated by `routine_header_requires_as`, an exhaustive per-dialect `match` true only for SQL
Server, because MySQL's routines have no header `AS` and PostgreSQL's is optional
(`BEGIN ATOMIC` has none) — there the first `AS` may sit inside the body.
**T-SQL's name prefixes reach the tokens.** `tokenize_range` keeps a `@`/`@@`/`#`/`##` with
the name it prefixes (`sql::t_sql_name_prefix`) and runs a name on through the bytes its
dialect continues one with (`sql::continues_name`); `#t` read as `t` was ``Table `t` not
found``, and `@curdate` read as `curdate` a misspelled keyword (`typo_checks` scans the same
way and skips a prefixed name). **Nor does either typo check judge an ODBC escape's
function** (`names_an_odbc_function`): `{fn UCASE(x)}` names ODBC's vocabulary, which SQL
Server runs as written (`UCASE`, `LCASE`, `CURDATE`, `IFNULL`, `LOCATE`, `TIMESTAMPADD` measured
on 2022) and a driver translates elsewhere, and each was a misspelled keyword or function
(`an_odbc_escape_function_is_not_a_misspelling`). **Nor does `typo_checks` judge a name the
statement declares with `AS`**, where it is declared or anywhere it is used: `COUNT(*) AS likes
… ORDER BY likes` was two misspelled `LIKE`s. The names are read off the statement's tokens
first; an implicit alias (`FROM t likes`) is still checked, since there `likes` may as well be a
mistyped `LIKE` (`an_alias_declared_with_as_is_not_a_keyword_typo`). `is_session_source` exempts a temporary table, a table
variable and `pseudo_tables` — T-SQL's `inserted`/`deleted`, which every trigger reads and the
typo check also exempts — from the table check, which cannot judge absent what lives in the
session; the per-dialect list is empty where trigger rows are `NEW`/`OLD` records, which
completion's scope supplies instead (`trigger_rows_are_records`, below). A T-SQL
write's target may name the alias its own `FROM` defines (`UPDATE e SET … FROM employees e`),
so `write_target_at` exempts that target alone, where `write_target_may_name_an_alias` (an
exhaustive per-dialect `match`, SQL Server's alone — MySQL's multi-table `UPDATE` names
tables); exempting every bare reference spelled like any alias in the statement hid a missing
table on all four engines (`only_a_write_target_may_name_its_own_alias`). With the
sigil kept, `as_declares_a_type` passes the `AS` of `DECLARE @t AS TABLE (…)` (`TABLE` is
reserved): the word before it is a variable and the nearest statement head is `DECLARE`,
`CREATE` or `ALTER` (`declaring_contexts`, one forward pass), so `SELECT @x AS order` is still
flagged (`t_sql_prefixed_names_are_exempt_and_nothing_else_is`).
**A T-SQL three-part name is `database.schema.table`**, and the scanners took one optional
`.name`, so `company.dbo.employees` was ``Table `dbo` not found in `company``` and
`tempdb..#t` the table `tempdb`. `dotted_name` reads every part (`..`'s missing schema an empty
one); `located_table_refs` keeps a three-part name's database beside the reference
(`LocatedRef`), and `Catalog::in_database` judges it — in the active database as the
schema-qualified (or, for `..`, bare) name, in another loaded one as that database's table of
the same name whatever its schema (which can miss a wrong schema but never invent a missing
table), and not at all in one never loaded. `colres` asks the same of the AST's parts, and a
linked server's four-part name is judged by nothing (`a_three_part_name_is_database_schema_table`;
the other-database arm, `a_three_part_name_in_another_loaded_database_is_judged_there`).
Completion's mid-edit scope reads the same parts through the same list reader (above), and
**both of completion's scope readers keep the database, not the schema** — `lexer_scope`'s
`emit_in` and the parsed `ast_scope::table_ref_of` alike: `company.dbo.employees e` is
`TableRef { name: employees, alias: e, db: Some("company") }`, and `company..employees` keeps
`company` too. They kept `dbo` (and `..` kept nothing), and `rank`'s `cols_of` looks a
reference up in `columns_by_db` first and then in `columns_by_schema`, which holds the tab's own
database only — so `archive.dbo.orders o` then `o.` offered the *tab's* `dbo.orders` columns
(`another_databases_three_part_table_completes_its_own_columns`, through `statement_scope`
composed with `rank`). Now it finds `archive`'s, and a database never loaded finds nothing —
unless the tab's database has a schema of that name, which the fallback then reads. The cost:
the scope reference drops the schema, so two same-named tables in different schemas of another
database merge, as `columns_by_db` already merges them. A four-part name registers nothing in
either reader. Diagnostics are untouched by this — they read `located_table_refs`, which was
already database-aware.
**A trigger's `NEW` and `OLD` are rows of its table, and completion's scope says so.** Neither
reader saw them: a `CREATE TRIGGER` gives the parsed reader no tables (or does not parse
mid-edit), and the lexer's scope of `… ON orders FOR EACH ROW SET NEW.` on MySQL was empty, so
`NEW.` completed nothing. `statement_scope` now takes the parsed scope when it has tables or
CTEs and the lexer's otherwise, as before, and then — where `trigger_rows_are_records` (MySQL,
PostgreSQL, SQLite; an exhaustive `match`, false for SQL Server, whose rows are the
`inserted`/`deleted` tables of `pseudo_tables` and which has no `NEW`) — appends the trigger's
table twice, aliased `new` and `old`, skipping a reference already identical. `trigger_table`
reads it off the statement's tokens: an unquoted `CREATE` first, an unquoted `TRIGGER` within
the first 16 tokens (room for `OR REPLACE`, `TEMP`, a `DEFINER = …`), the first unquoted `ON`
within 48 tokens after it (room for PostgreSQL's `UPDATE OF col, …`), and the `[db.]table`
after that, quoted parts allowed. So MySQL's `SET NEW.`, a SQLite body's `OLD.` and
PostgreSQL's `WHEN (NEW.` complete the table's columns through `rank`'s ordinary alias path,
with nothing in `rank` changed (`a_triggers_new_and_old_are_its_tables_row`, red before; the
composition, `rank::tests::a_triggers_new_completes_its_tables_columns`). The two aliases are
in scope for **every** `statement_scope` caller inside a trigger statement, join targets
included, not for completion alone. Diagnostics are untouched — they read
`located_table_refs`, not this scope. **An upsert's proposed row is added the same way**
(`upsert_row_alias`): an `INSERT INTO t …` that has, at its own paren depth, an `ON CONFLICT`
on PostgreSQL or SQLite gets `t` aliased `excluded`, and one on MySQL with an `AS n` between
its `VALUES` and `ON DUPLICATE KEY` gets `t` aliased `n` — an `AS` inside the rows (a cast's)
is not it. SQL Server's upsert is a `MERGE`, whose source is an ordinary reference, so it gets
none (`an_upserts_proposed_row_is_its_targets_row`, red before). The `[db.]table` reading the
two share is `table_name_at`.
**The parsed scope is the queries around the caret, not the statement.** `statement_scope`'s
parsed arm is `ast_scope::collect_at`: a visitor records every `Query` as a frame — its span
as buffer offsets (`colres::to_range`) and its *own* `FROM` tables, joins and nested joins
included but not a derived table's inside, which is that table's own frame — under a root
frame spanning the statement that holds the statement's own tables (an `INSERT`'s target, an
`UPDATE`'s target, joins and `FROM`, a `DELETE`'s `FROM` and `USING`). The scope is every frame
whose span holds the caret, **widest first**, so the innermost query's tables come last, where
completion's last-table bias wants them; reorder that sort and the bias points at the outer
query.
It was the union of every table in the statement, kept as "the safe direction": a subquery's
tables were offered in the outer `WHERE`, a CTE body's or derived table's inner table in the
outer select list, and `expand_star` over `(SELECT id FROM events) d` emitted every column of
`events`, which `d` does not expose — while `UPDATE … FROM` and `DELETE … USING` tables were
not read at all. Now a subquery in a `WHERE`, a select list or a `HAVING` contributes its
tables only with the caret inside it, and a CTE's or derived table's columns reach the outer
query through `local_relations` instead (`the_parsed_scope_is_the_queries_around_the_caret`,
`the_parsed_scope_reads_an_update_from_and_a_delete_using`;
`scope_resolves_cte_names_and_inner_tables` and `scope_recurses_into_derived_table` had pinned
the union and were rewritten). Every CTE name
is still kept. `ast_scope::collect_statement`, the flat walk, stays for the table check's CTE
set, which wants the whole statement's; the lexer fallback for a statement that does not parse
is unchanged.
The error's remedy
names the engine's quote (backticks, double quotes, brackets); it said "backticks" on every
engine, advice neither PostgreSQL nor SQL Server accepts
(`a_reserved_alias_error_names_this_engines_quote`).
**A SQL Server synonym is a relation to the catalogue.** It lives in `DbSchema::tsql_objects`,
not `tables`, so `SELECT id FROM dbo.docs_syn` was ``Table `docs_syn` not found in `dbo``` under
a synonym the schema tree listed one row away (seen in the app on 2022). `Catalog::build` reads
`DbSchema::synonyms` (under `schema.rs`): one resolved to this database's own table or view goes
into `qualified`, `schema_qualified` and `unqualified` with the **target's** columns, so a wrong
column through it is still reported; one that is not — another database, a linked server, a
routine, an ambiguous bare target — goes into `opaque_qualified` (keyed under its database and
its schema both, as the other two maps are) or `opaque_unqualified`, which `table_status` answers
`Found` while `columns_of` stays `None`, and the column checks read that as can't-judge. That is
the three-part name's rule again: a relation whose columns are unknown may let a wrong column
through but must never report a real one missing
(`a_synonym_is_a_relation_with_its_targets_columns`, red before). **A table or view the
document creates itself joins the same opaque maps for that run**: `diagnostics` reads
`created_tables` — each `CREATE [TEMPORARY] TABLE`, `CREATE VIEW` and table-making
`SELECT … INTO` (SQL Server's and PostgreSQL's; MySQL's fills variables), statement by
statement — and, only when there are some, checks against `Catalog::with_created`, a copy
with each one found and its columns unjudged, rather than reading columns out of the
`CREATE`, which would be a second schema reader. Order is not asked, so a reference above the
`CREATE` is quiet too, the safe direction (`a_table_the_script_creates_is_not_reported_missing`,
red before). A creation is registered **under every spelling that reaches it** — `CREATE TABLE
dbo.t` as a bare `t` too when `dbo` is the active database or one of its namespaces, and a bare
`CREATE TABLE t` under the active database and every namespace — since keyed only as written,
T-SQL's ordinary `CREATE TABLE dbo.t` then `FROM t` was "Table not found"
(`a_created_table_is_found_qualified_or_bare`, S8.1-L1-01, red before). And **the introspected
table of the same name is taken out of the column maps**, so a script that drops and makes
`employees` again is not judged by the old columns: `columns_of` reaches only the maps, so the
opaque entry alone left every new column "not found" (`a_recreated_table_is_not_judged_by_its_old_columns`,
S8.1-L1-04, red before). `created_tables` parses only a statement that could make a table —
the *word* `CREATE`, or an `INTO` not after `INSERT`/`MERGE` (`may_create_a_table`); a
substring test parsed every `created_at` and every `INSERT INTO`, three quarters of a warm
cache pass (S8.1-L4-01). By the same rule a resolved
synonym's columns are **merged** into a same-named table's `(database, name)` and bare-name
entries, never written over them, since `dbo.orders` replacing `sales.orders`' entry made the
correct `SELECT total FROM hist.sales.orders` a "Column not found"
(`a_synonym_sharing_a_tables_name_adds_to_its_columns`, S4.2-L1-04, red before). Same-named
**tables** take the same `merge`: the bare-name map always did, but `(database, name)` was
replaced, so `hist`'s `sales.orders` and `dbo.orders` kept whichever was indexed last and the
same three-part reference was the same false error with no synonym in sight
(`same_named_tables_in_another_database_merge_their_columns`, CMP-06, red before).
**The union is the right side for a diagnostic to err on and the wrong one for a prediction**,
which would write it out as one table's columns or keys. So `merge` also answers whether the
list it was handed differed from what the entry already held, and `Catalog::unioned` records
each entry that did — keyed `(qualifier, table)`, `""` for the bare map, as `insert_skips` is.
Two relations with the same columns are not a union of anything. `Catalog::is_union` asks it
of a reference, and what the predictions do with the answer is under `predict` below.
**`NON_RESERVED_KEYWORDS` is legal keywords one edit from a curated one**, which the typo
checker would otherwise flag. It is dialect-free, since each is legal on some engine here and
which engine has which is `builtin_catalog`'s half of the question. The list was widened by
measurement, not memory: sqlparser's `ALL_KEYWORDS` run through the checker on each engine found
`AFTER` flagged on all four (one edit from `ALTER`), `SHOW TABLES` on MySQL and `GROUPING SETS`
on PostgreSQL, among others (`a_legal_keyword_one_edit_from_a_curated_one_is_not_a_typo`). The
other engines' keywords that run also flagged (`ZORDER`, `ILIKE` on MySQL) stay squiggled.
**The typo checker runs only where the app holds the engine's own catalog** — `builtin_catalog`
answers that with one exhaustive `match` **returning the catalog itself**: `FUNCTIONS` for
MySQL, `SQLITE_FUNCTIONS` for SQLite, `PG_FUNCTIONS` for PostgreSQL, `MSSQL_FUNCTIONS` for SQL
Server, and `is_known_function` and
`is_probable_function_typo` take it as a parameter — as the `CatalogIndex` built from it rather
than as the slice itself, which is the same catalog reached one hop further on. Reaching for the
module-level `FUNCTIONS`
inside those two was the defect under the original bug: the checker spent its `dialect` on
`skip_noncode` and `is_sql_keyword` and never on the catalog questions, so on PostgreSQL it
failed to recognise the engine's real functions *and* measured its edit distances against another
engine's, and `btrim`, `to_number`, `make_date` and `make_time` — core builtins, verified on PG
16.15 — were all four squiggled as misspelled. Losing a nicety on an engine beats the app calling
correct SQL broken. A `builtin_functions_are_authoritative` bool stood beside the catalog and has
been **deleted**: returning the catalog *is* answering whether this app's builtins are
authoritative here, and asking one question at two sites is one more place to forget an engine.
**`rank::rank` is a second caller, and the surfaces that ask are named rather than counted** — the
completion popup went on walking `FUNCTIONS` on every engine for as long as the checker had been
fixed, so a PostgreSQL tab was offered `IFNULL` and `GROUP_CONCAT`, names that engine does not
have, and never `btrim`, which it does; SQLite was handed MySQL's list just as silently. That is
the original bug in the surface that *inserts* the word rather than the one that underlines it,
and suggesting a name the engine lacks is the same wrong as squiggling one it has, so every such
surface asks the one engine→catalog map rather than each keeping its own, and `None` means
*offer nothing* on the same reasoning that makes it mean *say nothing*. **Counting them is what
went wrong**: this passage said there were two while `signature_help` still named `FUNCTIONS`
outright, so a PostgreSQL tab got no help at all for `btrim(` and got MySQL's
`DATE_FORMAT(date, format)` for a function that engine has not got —
`signature_help_answers_from_the_engines_own_catalog` asserts both halves, since "shows nothing"
and "shows the wrong engine's" are one defect with two faces. `rank` reaches the map through
`offered_builtins` now rather than calling `builtin_catalog` itself, which leaves the map with
no caller outside this module and is the same "one place to forget an engine" one hop on.
`builtin_summary(dialect, name)` is the fourth asker and keeps that shape: the completion
popup's footer (under `completion.rs`) wants the selected function's one-line `summary`, and
it reads it out of `builtin_catalog(dialect)` — the catalogue signature help reads, matched in
any case — so a name the engine lacks answers `None` rather than another engine's description
(`a_builtins_summary_is_found_by_name_in_its_engines_catalogue`, which asks `DATE_FORMAT` of
PostgreSQL for exactly that).
**`is_offered_builtin` is where those two surfaces part company again**, because trusting a name
and offering it are not the same question: `builtin_catalog` is what the checker **trusts**,
whole, and `is_offered_builtin(dialect, flavour, name)` is what `rank`'s `ClauseCtx::Column` arm
filters it through before the popup sees it. MySQL's, SQLite's and SQL Server's
catalogs were transcribed from their manuals and each already *is* the list a person would type,
so SQLite's and SQL Server's arms are `true` outright and MySQL's is `true` but for the flavour
split below;
PostgreSQL's was read out of `pg_catalog`, so it carries the engine's
own plumbing — `int4in`, `btint4cmp`, `texteq` — which the checker must keep knowing and the
popup must not spend its forty rows on, and that arm asks `pg_builtins::is_suggested`.
**It is asked once per dialect and flavour rather than once per candidate**: the answer is
`CatalogIndex::offered`, and `offered_builtins(dialect, flavour)` is the slice `rank` iterates.
`is_suggested` is a binary search, and running it over all 2,706 entries to arrive at the same
863 names every time was 2,706 binary searches per keystroke on a path with no debounce.
**It is an exhaustive `match` on a dialect, and that is not the engine comparison the working
rules warn against**: what it asks is a property of how *this app's* data file was made, not a
server capability, so there is no yes/no behind it to compute from and a fourth engine's answer
depends on how its own catalog was written. A `dialect == Postgres` at the call site would be
the failure; one named predicate, here, with a reason per arm, is what keeps the question
answerable — the same argument `ddl::default_new_column_type` makes for its own `match`.
**`SQLITE_FUNCTIONS` turned the checker on for SQLite** — 157 entries, written lower-case the way
SQLite's own documentation writes them while `FUNCTIONS` stays upper-case for MySQL's, which
costs nothing because both comparisons are case-insensitive at either end.
**PostgreSQL has one now, and a hand-written list was never what could give it one**: the
checker only ever speaks about a near miss of a name it *holds*, so a partial catalog
reintroduces the original false positives for whatever it omits — `to_date` squiggled because
`to_char` made the list. SQLite could be written out by hand because its builtins are a small
closed set its documentation enumerates; PostgreSQL's are not, and the honest source for them is
`pg_catalog` on a real server, which is what `pg_builtins.rs` below was *generated* from. **The
`Option` outlives that**: all four engines answer `Some` today, and the `None` arm stays for a
*fifth*, which has to land on it rather than inherit whichever list is nearest — the original
bug, one engine further on. **SQL Server was the fourth, and it landed on `None` first**: no
completion builtins, no signature help and no misspelled-function checker until a catalog could
be checked against a server as PostgreSQL's is. `mssql_builtins.rs` below is that catalog, and
what it is checked against is the server's parser, since T-SQL's builtins are in no catalog view
to diff it with. Its reserved words (`MSSQL_RESERVED`, the documented list) are there, **less
six the parser takes as names anyway** — `DISK`, `DUMP`, `LOAD`, `PRECISION`, `SECURITYAUDIT`
and `WITHIN` ran as column alias, table alias and column name on SQL Server 2022 and 2025, and
the system views' own `t.prec AS precision` was a red reserved-alias error. Those six are
`alias_ok_but_unquotable`'s on this engine, so the quoter still brackets them; the live leg
`the_reserved_list_is_what_the_server_refuses_as_an_alias` compares every documented word's
`is_reserved_word` with the server's answer to `SELECT 1 AS <word>`.
**The checker's exemption set is the other half of "a name this engine really has", and an
extension's functions fell straight through it.** `function_typo_checks` passes anything in the
catalog's `known_idents` — databases, tables, columns, namespaces, the schema's stored
routines and SQL Server's synonyms — and that set is built from what the tree lists. `pg::routine_filter`
deliberately keeps an extension's functions *out* of the tree (PostGIS alone installs ~1,000
`st_*` into `public`, and that decision stands), so an extension function was not merely absent
from the tree: it was **squiggled as a misspelling under correct SQL**, which is the exact
failure that switched this checker off for PostgreSQL in the first place. Measured on PG 16.15
with `citext`, `intarray`, `cube`, `earthdistance`, `tablefunc` and `hstore` installed, **19 of
the 196 extension-owned names** came back "looks like a misspelled function" —
`earth_distance`, `icount`, `sort`, `tconvert` and fifteen `citext_*`.
`DbSchema::extension_routines` is what closes it: **names and nothing else**, PostgreSQL only,
folded into `known_idents` beside `schema.routines` by `Catalog::build`. It answers the
checker's question — is this a real callable name here — without reopening the one the tree
settled the other way, and without shipping a thousand bodies over the wire to do it.
`an_extension_function_is_a_known_name_even_though_the_tree_omits_it` asserts its own **premise
first**, that those five names really do near-miss when nothing carries them, so it cannot come
to pass by the population having moved; the live half is `live::pg_catalog`'s
`an_installed_extensions_functions_are_names_the_checker_knows`, below.
**`an_ordinary_user_function_survives_the_pg_catalog` was sampling the wrong population**, and
that is how this sat unseen behind a test that stated the cost property: its ten names were
9-to-14-byte compounds, and at that length the near-miss threshold is 2 and a compound is far
from everything, so all ten passed by construction. Nineteen short single-word names were added
— 4–7 bytes, threshold 1 — and all nineteen pass too. The useful result is the one that is not
a fix: the failing population was never "short user names", it was extension names.
**A catalog is data, so the engine checks it** — `schemaic-db/tests/sqlite_catalog.rs`, which
lives over there rather than beside the catalog because `schemaic-core` does not link rusqlite, and needs
no server, no file and no network (the in-memory-SQLite allowance).
`every_function_the_engine_reports_is_in_the_catalog` asks `pragma_function_list` on an in-memory
connection — the engine's own answer to the same question, rather than anyone's memory of the
documentation — and fails on any name the engine has and the list lacks, because that is a
squiggle under correct SQL. Run against the hand-written list it found **23** missing names: the
whole R-tree family (`rtreecheck`, `rtreedepth`, `rtreenode`), the FTS3/5 family (`matchinfo`,
`offsets`, `optimize`, `fts3_tokenizer`, the `fts5_*` set),
`json_array_insert`/`jsonb_array_insert`, `unistr`, `subtype`, `sqlite_log` and
`current_date`/`current_time`/`current_timestamp`. It also settled that SQLite has `if()` as an
alias of `iif()`, which the documentation does not lead with. **The check is one-directional on
purpose**, and `over_listing` says so rather than leaving the asymmetry to be discovered and
"fixed": this build has `SQLITE_ENABLE_MATH_FUNCTIONS` off, so the engine reports none of the 29
math functions nor `sqlite_offset`, and `pragma_function_list` reports no table-valued function,
so `json_each`/`json_tree` never appear either. Those stay listed, because a `.db` is a file
other tools open and squiggling `sqrt(x)` to describe a local compile flag is the false positive
the catalog exists to prevent; `over_listing` asserts only that the overhang stays *explicable* —
every name in it belongs to one of those named families. Both tests skip `sqlite_rename_*` (the
rename machinery `ALTER TABLE` drives, not callable API, and names no user should be nudged
toward) and the `->`/`->>` operators, which the checker cannot reach since it only looks at a
word followed by `(`.
**PostgreSQL's half of that same rule is `live::pg_catalog`**, which re-asks the server under
test the query that generated the file and fails on any name `PG_FUNCTIONS` lacks. It sits in the
live tier rather than beside the catalog because its oracle is a *server*, where SQLite's is a
linked library.
**MySQL/MariaDB's half is `live::mariadb_catalog`, and until it landed `FUNCTIONS` was the one
catalog of the three with no oracle at all** — SQLite's answers to the linked library,
PostgreSQL's to `pg_catalog`, and the list the two engines that ship most are checked against was
written from memory of the documentation with nothing behind it. It found **54** names MariaDB
10.11 really has and the list lacked, every one of them a squiggle under correct SQL on the
engine this app was built for first — the same class of bug that switched the checker off for
PostgreSQL, sitting unnoticed on the default engine rather than the newest one. `FUNCTIONS` grew
by **57** entries: those 54, plus three dynamic-column forms added so that family is listed whole
rather than half, half a family being worse for the completion popup than either all of it or
none. Three family sections are new — **Dynamic columns (MariaDB)**, **Oracle-mode variants
(MariaDB)** (`SUBSTR_ORACLE`, `DECODE_ORACLE`, `LPAD_ORACLE` …, callable in any `sql_mode`, which
is why the server reports them and the checker must not squiggle them) and **Replication &
cluster** (`MASTER_GTID_WAIT`, `BINLOG_GTID_POS`, the `WSREP_*` trio, `DECODE_HISTOGRAM`) — plus
a small **XML** one (`EXTRACTVALUE`, `UPDATEXML`).
**`FUNCTIONS` is one catalog for two servers, and that was free until it reached the popup.**
`rank.rs` reaches this catalog through `builtin_catalog` rather than naming it, so the overhang
stops at the two engines that share it instead of reaching every tab — which it did, and which
is the bug above. But a name MariaDB has and MySQL 8 has not is still a row a MySQL tab cannot
call, and `SqlDialect` cannot say which of the two is in front of the tab: it has no MariaDB arm
on purpose, which is right for parsing, quoting and the *shape* of a completion and wrong for
one thing only — which names exist.
**Measured, not remembered.** Every name in `FUNCTIONS` was executed on both servers
(`SELECT <name>(1,2)` with a database selected; `ERROR 1305` means the server has no such
function, any other error means it has one and was called wrongly). MySQL 8.4.11 answered 1305
for the eight `COLUMN_*` dynamic-column functions, the nine `*_ORACLE`
spellings, the three `WSREP_*`, `NVL`/`NVL2`/`TO_CHAR`/`ADD_MONTHS`/`SYS_GUID` from Oracle mode
and the rest — forty-nine at that pass, **122** since the spatial families landed — and MariaDB
10.11.14 for exactly **5** (`BIN_TO_UUID`, `IS_UUID`,
`JSON_STORAGE_SIZE`, `REGEXP_LIKE`, `UUID_TO_BIN`). Those are `MARIADB_ONLY` and `MYSQL_ONLY`,
sorted for a binary search and `pub` for the live oracle alone. **Two different reasons live in
`MARIADB_ONLY` and the list is right for both**: most are MariaDB's own, but `DES_ENCRYPT`,
`DES_DECRYPT`, `ENCODE`, `ENCRYPT` and `OLD_PASSWORD` are functions MySQL *removed* in 8.0 and
MariaDB kept — the question the list answers is "can the server in front of me call it", and for
MySQL 8.4 the answer is no either way. `MYSQL_ONLY` is short *because* the catalog was
transcribed from MariaDB's manual: the asymmetry is a property of how this file was written, not
of the two servers.
**Only the offer narrows; the checker goes on trusting the catalog whole**, which is the
PostgreSQL split one engine along: squiggling `UUID_TO_BIN` for the MySQL user who typed it is
the false positive this area exists to prevent, so neither list is subtracted from what
`builtin_catalog` returns.
**`ServerFlavour::Unknown` offers everything, and that is deliberately *not* the
withhold-on-uncertainty rule `ServerFlavour::is_mariadb` follows.** That rule is for writes,
where guessing costs a table's constraints; here guessing costs one extra row in a popup while
withholding costs 54 names on every tab whose schema has not finished loading. The flavour only
ever *narrows* once it is known. `every_flavoured_name_is_in_the_catalog` holds both lists
sorted, really in `FUNCTIONS`, and not overlapping;
`a_tab_is_only_offered_the_builtins_its_own_server_has` asserts the consequence **through
`rank`** rather than over `is_offered_builtin`, because the predicate reading correctly is not
the property — the popup not offering the name is, and `CatalogIndex`'s precomputed per-flavour
lists sit between the two. It asks with a *proper prefix* of each name, since `rank`'s dedup
drops a candidate equal to what is already typed.
**The spatial families are the largest thing this catalog has been missing, and the reason
nobody saw them is a claim about `SQL_FUNCTIONS` that was false.** `FUNCTIONS` holds **463**
entries now: 83 `ST_*` names callable on MySQL 8.4 and MariaDB 10.11 alike, and 73 MariaDB-only
spellings — the 69 unprefixed OGC-v1 synonyms MariaDB kept and MySQL 8 removed, plus
`ST_BOUNDARY`, `ST_ISRING`, `ST_POINTONSURFACE` and `ST_RELATE`. Until they landed `ST_Length`
squiggled under correct SQL on the engine this app was built for first.
`information_schema.SQL_FUNCTIONS` is **not** "every builtin the parser registers by name": a
builtin registered natively and resolved by its own path is in neither that view nor
`KEYWORDS` — MariaDB answers `SELECT st_area()` with *"incorrect parameter count in the call to
native function 'st_area'"* while the view does not list it — so the whole OGC family sat in
that gap with every test in `live::mariadb_catalog` green. Anywhere the union of the two server
views is described as what the server has, that is the false claim; the third oracle is the
parser.
**`DEPRECATED_ALIASES` is the MySQL family's answer to `pg_builtins::is_suggested`, and it is
the question `MARIADB_ONLY` could not be asked.** Those two lists say which *server* has a name;
nothing said whether a name the server has is worth *suggesting*, and across 309 entries the
distinction never arose because every one of them was a name somebody types on purpose. The
OGC-v1 synonyms are not. They are real, callable functions — squiggling `GLENGTH` under correct
SQL is the false positive this whole half exists to avoid, so the catalog has to carry them —
but they are spelled `X`, `Y`, `AREA`, `CONTAINS`, `EQUALS`, `OVERLAPS`, `WITHIN`, `BUFFER`, the
most ordinary column names there are, and MariaDB documents every one as a synonym for the
`ST_`-prefixed name. Offering them puts `X(` in the popup of everyone who typed `x`. So: in the
catalog, out of the suggestions — **69** names, sorted for a binary search, consulted by
`is_offered_builtin`'s MySQL arm *before* the flavour and for every flavour **including
`Unknown`**, because a name nobody means is not worth offering on the one server that still has
it either. `every_deprecated_alias_is_in_the_catalog_and_none_is_offered` holds both halves at
once: a name that drifted out of `FUNCTIONS` would withhold nothing *and* start being squiggled
again — the false positive arriving back through the list written to describe it — and an
unsorted list answers "not in it" for names that are in it, which puts `X(` and `Y(` back in the
popup.
**A builtin's name plus a `_` or a digit is a *derived* name, not a misspelling**, and
`is_probable_function_typo` refuses those outright. Its own doc already claimed the design
avoided flagging `format_x` as a typo of `FORMAT` — by not loosening the distance threshold —
but at length 8 the threshold is already 2, so it flagged it anyway, and `coalesce_x`,
`concat_ws2`, `instr2` and `md55` with it, all measured. Nobody misspells `FORMAT` by typing
every letter of it and then an underscore. **The separator is what keeps the rule narrow**: a
*letter* continuation is how a real typo looks, `SUBSTRIN` being `SUBSTR` plus `IN` and a dropped
`G` from `SUBSTRING`, so those are still flagged. It is the same shape as the transposition case
the function already carries — a named exception rather than a wider distance, because widening
the distance is what produced this class in the first place.
**The catalog is read through a `CatalogIndex` now — one per dialect, built on first use — and
turning the PostgreSQL checker on is what forced it.** Both of the checker's loops used to walk
the slice, mapping it to an upper-cased `String` per entry, twice over, per candidate word, per
statement, on the editor's 120 ms debounce — affordable against MySQL's catalog (309 entries
then, 463 now) and not
against `PG_FUNCTIONS`' 2,706 — while the completion path walked the same slice undebounced. The
index is a *rearrangement* of that slice and nothing else: nothing added, nothing filtered, so
every answer is the answer the walk gave and only the cost differs. `lower` is every name
lower-cased, which makes `is_known_function` an allocation-free `O(1)` probe with the word the
caller has already lower-cased for its own `known_idents` lookup — the walk it replaces ran
`eq_ignore_ascii_case` against all 2,706 entries to answer "no", which is the answer for every
user-defined function in a buffer. `upper` is the same names upper-cased, probed as a **prefix**
for the derived-name test and so asked from the *word's* side: for each `_`-or-digit byte in the
word, is everything before it a name? That is one set lookup per separator byte instead of a
walk of the catalog per word. `by_len` buckets the upper-cased names by byte length, each paired
with a character-presence mask, and `offered` is `is_offered_builtin`'s answer precomputed —
**`[Vec<&'static SqlFunction>; 3]`, one list per `ServerFlavour`**, because on the MySQL arm
that answer depends on which of the two servers the tab is on. `CatalogIndex::slot(flavour)` is
a `match` rather than an `as usize`, so a fourth flavour is a compiler error instead of an index
quietly reading somebody else's list. Three short vectors rather than a fourth `OnceLock` keyed
on the (dialect, flavour) pair: the flavour changes what is *offered* and nothing else in the
struct, so splitting the whole index would rebuild the buckets and both hash sets three times
over to vary one field. On the other three dialects all three lists are the same.
**Four `OnceLock`s, one per `SqlDialect` arm, rather than one map keyed by the dialect** — the
set of engines is closed at compile time, so a fifth engine is a compiler error in the cache
too rather than a silent miss at runtime, which is `builtin_catalog`'s own argument one layer on.
**`CatalogIndex::within(len, thresh, mask)` is the candidate set, and what makes skipping the
rest sound is that both of its conditions are *necessary*.** Neither is sufficient and neither is
asked to be: the same `edit_distance`/`is_adjacent_transposition` pair still decides, on a
smaller set. Length is the first — `edit_distance(a, b) >= |a.len() - b.len()|`, and
`is_adjacent_transposition` is `false` outright on a length mismatch — and it is the filter the
linear walk already had, indexed instead of scanned. Characters are the second:
`(mask_a ^ mask_b).count_ones() <= 2 * thresh`, because each edit moves the presence set by at
most two bits (a substitution can drop one character and add another, an insertion or deletion
at most one), so `thresh` edits cannot separate two masks by more than `2 * thresh` bits; a
transposition reorders and changes no bit at all, which is why one test covers both arms.
**The second condition is the one that did the work, and the obvious fix was the first alone.**
Bucketing by length reads like the whole answer — a 9-byte word visits buckets 7 through 11
rather than 2,706 entries — but the data does not cooperate: MySQL's names spread over 2–21
bytes while PostgreSQL's 2,706 cluster hard in 5–12, so those five buckets are still most of the
catalog. Length bucketing alone was measured at **1.6×**; the mask is what turned it into two
orders of magnitude.
**`letter_mask` gives everything that is not an ASCII letter, digit or `_` one *shared* bit, and
the sharing is load-bearing rather than lazy.** `sql::is_word_byte` treats every byte `>= 0x80`
as a word byte, so a word with multi-byte characters in it really does reach the checker, and two
different such characters landing on one bit cannot make the popcount *larger* than the edits
between them allow — which is the only direction that could lose a match. Spending a bit per
distinct byte instead would read as more precise and would break the necessary condition.
**Measured, release build, over a 300-statement PostGIS-shaped buffer** — statements calling
extension functions the catalog does not hold, which is the population that reaches the scan.
`function_typo_checks` went from 424.6 ms to 4.3–4.5 ms per tick on PostgreSQL and from 28.6 ms
to 1.2 ms on MySQL; the whole of `diagnostics` from 393.0 ms to 21.5–22.6 ms on PostgreSQL and
from 48.0 ms to 18.1–22.9 ms on MySQL. The debounce is 120 ms, so PostgreSQL went from three and
a half times over its own tick budget to inside it with room to spare. **The two "before"
figures do not decompose, and that is the finding rather than an error in them**: 424.6 ms of
scan inside a 393.0 ms `diagnostics` is one number measured twice to within the run-to-run
spread, because the catalog walk *was* substantially all of the cost — parsing 300 statements
included. Afterwards the scan is 4.3 ms of a 21.5 ms whole, which decomposes as it should.
**`the_bucketed_catalog_agrees_with_a_full_walk_of_it` is what holds all of that, and the
property dictates its shape**: "same answer, fewer entries examined" is only checkable against
the walk that was replaced, so the test carries a transcription of that walk and asserts
agreement across all three catalogs, asserting nothing about wall-clock time. The corpus is
sampled **three names per length class, not per entry** — a length class is precisely the unit a
window error can lose — and the test asserts that its sample covers every occupied bucket
(`per_len.len() == occupied`) plus a floor on the corpus size, so a bucket map that quietly came
back with one entry cannot leave it agreeing with itself over nothing. One liberty is taken with
the transcription and it is named there: the per-comparison `to_ascii_uppercase()` is hoisted to
the caller, because what is under test is which entries are considered and how each is compared
rather than the allocation, and leaving it in put the test at 36 seconds. It was watched
failing — `within`'s `lo` changed to `lo + 1` gives `MySql disagrees about "SUMs" (around SUM)`,
every single-insertion typo of a catalog name living in exactly the bucket an off-by-one drops —
and it runs in 0.68 s. **A linear filter cannot be got wrong that way**, which is the price of
the speed and the reason the test is there.
`a_multi_byte_function_name_is_answered_not_panicked_on` is the other half: the derived-name test
slices `up[..n]` on a `String` where the walk sliced bytes, and a slice off a char boundary is a
panic rather than a wrong answer. It is safe for a reason and not by luck — `n` is the index of
an ASCII `_` or digit, and the byte before an ASCII byte is always a char boundary — which is
exactly why the reason is worth pinning. And three existing tests moved from
`is_known_function(<slice>, name)` to a `catalog_knows(dialect, name)` helper that goes through
the index the checker itself reads, so a dialect wired to the wrong catalog *or* an index built
from the wrong slice fails them the same way.
**`diagnostics` runs over the whole document 120 ms after a keystroke, with no size cap, and two
things in it were redoing work the statement could not change.**
`static_words` is every keyword and builtin name this crate knows, lowercased, behind a
`OnceLock` — it was rebuilt inside `typo_checks`, which runs once per statement per tick, so
~700 `String` allocations per statement per tick for a value that depends on neither the schema,
the dialect nor the SQL. And `table_existence_checks` now takes the `asts` its caller already
has rather than reaching for the CTE names through `statement_scope`, which **re-parses the
identical statement** and, on a parse failure, tokenizes it a second time on top — everything
but `.ctes` then thrown away. Measured at 500 statements, that redundant parse was 10% of an
`INSERT`-heavy pass and 18% of a `SELECT … JOIN … ORDER BY` one; 72 ms at 98 KB, four dropped
frames while the pass still ran on the UI thread, on a file size that is an ordinary thing to
open here. The one call site is
`diagnostics`' `Ok(asts)` arm, so an AST is always in hand and the `Err` arm never reached this
function at all — `statement_scope`'s lexer fallback is not lost, only unreachable from here,
and it yields the same empty CTE list a `len() != 1` parse gives.
**The column resolver turns a span into bytes per expression, so that conversion is indexed.**
`colres::check` builds a `LineIndex` of the statement once — where each line starts, and
whether it is all ASCII — and every span the `Collector` places goes through
`Collector::range` → `colres::to_range_in`: a table read, plus a character walk only on a line
that holds a multi-byte character. `to_range` calls `offset_of_line_col`, which walks from the
text's first line on every call, so the pass was quadratic in the statement. Measured in a
release build, MySQL, empty catalog: a 4,000-row `INSERT … VALUES`, one row per line, went from
400 ms to 16 ms; one full `DiagnosticsCache::diagnostics` pass over a 7.5 MB script of 350 such
1,000-row `INSERT`s (350k lines) from 11–12 s to 1.4 s; and a 1.1 MB, 50,000-row `INSERT` on a
single line, as mysqldump writes them, from 113 s to 0.2 s — there the column walk inside the
line was linear too, and the ASCII flag is what removes it. `alias_refs::Walk` places its names
through `Walk::span_range` on the same index. The CTE walk (`Ctes`) and the free functions that
still call `to_range` keep it, since they convert a span per query or table rather than per
expression; a per-expression walk that reaches for `to_range` brings the quadratic back.
`the_line_index_answers_what_offset_of_line_col_does` holds the two to one answer. It was found
chasing a report of a 7 MB script taking the app to 3 GB: on main, a sandboxed release run over
the 350k-line script — Ctrl+End and an edit issued programmatically, wrap on and off, a live
connection — stayed near 400 MB with floem building under 1,000 text layouts. The memory half
did not reproduce; the CPU half was this.
**`DiagnosticsCache` is `diagnostics` remembered statement by statement**, so a keystroke
re-checks the statement it changed and reads the rest back; the editor keeps one per pane and
runs it on a worker thread (`editor_pane.rs`). Off T-SQL a range's answer is a function of its
own text, of whether it is the buffer's last (redundant in practice — only the last range can
be unterminated — but kept in the key), of the catalog overlaid with the document's
`created_tables`, and of whether a `USE` of another database stands above it. That last is
`UseScope`, which both passes walk down the script: below a `USE` other than the active
database, `Catalog::used_elsewhere` answers — bare and namespace-qualified names can't be
judged, since the catalogue never read that database's — and a `USE` back restores the active
one. Ignoring the `USE` made every table of the database it named "not found", while the run
paths follow it (`a_bare_name_below_a_use_of_another_database_is_not_judged`, S8.1-L1-05, red
before). So an entry is keyed by `(text, is_last, below_use)` and holds offsets relative to the
range's start — a line inserted above moves every statement and invalidates none — and every
entry is dropped when the catalog, the dialect or the created set changes. The catalog is
compared by `Arc::ptr_eq`, and **the basis holds the `Arc`** so a freed catalog's address cannot
be reused by the next one and pass as unchanged. The overlaid catalog is built once per basis
rather than once per pass, and entries the latest pass did not use are dropped, so the cache
holds one document's worth. **The created set is load-bearing in the basis**: adding or removing
a `CREATE TABLE` changes what every *other* statement's unknown-table check sees without
touching their text. **T-SQL passes straight through** to `diagnostics` and clears the cache —
its ranges share `TsqlFlow` state (an `IF`'s body, a `BEGIN`'s `END`, a batch's `GO`) held as
absolute spans, so no range's answer there is its own.
`the_diagnostics_cache_answers_what_the_whole_pass_does` drives one cache through typing inside
a statement, inserting above it, a `CREATE TABLE` added then removed, a catalog swap and T-SQL,
and asserts it equals `diagnostics` at every step; it was watched failing with the created set
left out of the basis.
**Two facts about a FROM source that the resolver was not recording.** `Src::shadowed` is the
table name an alias **replaced**: an alias is a rename for the whole query rather than a second
name, and MariaDB 10.11.14 answers `SELECT employees.id FROM employees e` with
`ERROR 1054: Unknown column 'employees.id'`, PostgreSQL the same. `quals` used to carry both
spellings, so that statement resolved clean — and dropping the bare name from `quals` alone only
made the qualifier *unmodelled*, which is deliberately not flagged (a `db.table.col` reference
reaches the same arm). Recording what the alias hid is what lets the difference be *stated*, in
a message of its own that names the alias to qualify with, and it is
checked only after the whole scope chain has failed so `FROM employees e JOIN employees` still
resolves. `Src::defines` is the byte range of the subquery a derived table or CTE is the body of,
and a reference **inside** that range must not resolve against it: the chain walks outwards so a
subquery can see an enclosing scope's tables, but never the source it is itself defining, and
`select_output_cols` names an unaliased projection item by the identifier itself — so the outer
source's columns *were* the misspelling. `SELECT nope FROM departments` was flagged and
`SELECT * FROM (SELECT nope FROM departments) d` was silent, which is the most common place for a
wrong column name to be.
**A recursive CTE is the one exception, and it is why the CTE map carries an `Option`.** `Ctes`
is `HashMap<String, (Cols, Option<(usize, usize)>)>`: the range is `None` for a `WITH RECURSIVE`
list, because a body naming itself is the whole construct there rather than an error. The rule
that is right for a derived table — where a self-reference is impossible — put two red Error
squiggles on `WITH RECURSIVE nums(n) AS (SELECT 1 … UNION ALL SELECT n + 1 FROM nums …)`, which
MariaDB 10.2+, PostgreSQL and SQLite all accept. **The exception is taken at the list, not at the
CTE**, because `RECURSIVE` marks the whole `WITH` list in the standard and not one member of it —
so a sibling in a recursive list also loses the check. `a_recursive_ctes_body_may_name_itself`
carries the counterweight in the same test: a **non**-recursive CTE still cannot see itself, and
a misspelling inside a recursive one is still a misspelling.
**The CTE map is scoped by a frame stack, the same shape `provenance_sources` got in `c83f920`**
— and this visitor had the identical bug with no `post_visit_query` at all. It was inserted into
and never removed from, and subqueries are visited in source order, so
`SELECT a.z, b.name FROM (WITH departments AS (SELECT 1 AS z) SELECT z FROM departments) a,
(SELECT name FROM departments) b` still had the first derived table's `departments` in the map
when the second was visited: `add_source`'s CTE arm takes a map hit **before** it asks the
catalogue, so the real table resolved against the CTE's one column and a valid statement was
squiggled ``Column `name` not found``. Each query pushes a frame recording what its own CTEs
displaced and `post_visit_query` puts the map back, so an outer CTE stays visible to an inner
query that does not rebind the name — restoring, not clearing.
`a_nested_ctes_name_does_not_leak_into_a_later_sibling_subquery` carries both counterweights,
since the fix must not simply stop shadowing.
**`UPDATE` and `DELETE` go through `colres` too, and an `INSERT`'s column list is measured.**
Only a `Query` reached the resolver, so a write got the flat `qualified_column_checks` alone and
an unqualified name in its `WHERE` or `SET` was never checked. `Collector::write_scope`, pushed
before the visit, gives the statement one scope over every table it reads — the target and its
joins, an `UPDATE`'s `FROM`, a `DELETE`'s `FROM` and `USING` — which `WHERE`, the `SET` values,
`RETURNING`/`OUTPUT` and `ORDER BY` resolve against and a subquery's references fall through
to. The `SET` *targets* are names rather than expressions, so the visitor never sees them; they
get a scope of their own — the target table alone where a `FROM` follows, since PostgreSQL and
SQL Server never read a `SET` column from it and `SET name = d.name FROM departments d` is not
ambiguous, and the whole join list on MySQL's `UPDATE a JOIN b SET`, which assigns to either.
A `SET @v` is skipped. **T-SQL's `UPDATE e … FROM employees e` names an alias as its target**:
read as a table of its own, `e` is unknown and so open to every column, and it answered for
`e.nope` — `t_sql_clauses_read_past_hide_nothing_beside_them` caught it — so a bare target
naming a `FROM` alias is replaced by that `FROM` item. **Over more than one source neither scope
checks ambiguity** (`natural`): doing it right needs a `USING` join's coalescing and the
per-engine reading of a target alias that lists the same table twice, and an over-eager check is
a red squiggle on valid SQL; an unknown column is still found. `pre_visit_expr` passes an
unquoted `DEFAULT`, which sqlparser reads in `SET x = DEFAULT` as a name. `insert_columns`
resolves an `INSERT`'s column list against `named_source` — `add_source`'s table-name arm,
extracted — so a table not loaded or not known stays silent. An `INSERT … SELECT`'s query goes
through the resolver too, its scopes the query's own; the upsert clauses outside it
(`ON DUPLICATE KEY UPDATE`, `ON CONFLICT … DO UPDATE`) resolve against no scope and keep the
flat qualified scan, and a name both report is one diagnostic by `dedup_diagnostics`
(`an_insert_selects_columns_are_checked`). Every other statement takes the flat scan only
(`an_unknown_bare_column_in_an_update_or_delete_is_flagged`,
`a_valid_update_or_delete_draws_no_column_flag`, `an_unknown_column_in_an_insert_list_is_flagged`).
**`dedup_diagnostics` is the last gate every offline diagnostic passes, and it had no test.** It
promises to drop what an earlier, *higher-or-equal severity* diagnostic already covers; the
severity half was only in the sort, and only at an equal start offset, so the coverage test read
no severity at all and a Warning that happened to start earlier swallowed a real Error inside it.
Not hypothetical: `cartesian_check` squiggles the whole derived table, so the unconstrained-join
diagnostic (then a warning, now a hint) hid the unknown column inside the subquery — the user was
told the join was unconstrained and not told the statement would fail. Making that check a hint
repeated the bug one severity lower, while the sweep still let any kept diagnostic cover a
warning: a `= NULL` inside the derived table lost its warning to the hint.
`a_warning_does_not_swallow_the_error_inside_it` and `a_hint_does_not_swallow_the_warning_inside_it`
pin the composition and `dedup_keeps_a_covered_diagnostic_only_when_the_cover_is_no_weaker` the
severity combinations, hints included, which is what says the fix is about severity rather than
about one statement.
It is **one sweep after the sort**: everything kept starts no later than the diagnostic in hand,
so that one is covered exactly when a kept error (for an error), a kept error or warning (for a
warning) or any kept diagnostic (for a hint) reaches as far, and three running maxima answer it. The sort's tie-break at an
equal start is `Severity::rank` (error, warning, hint), the same order the editor's hover
`diag_under` settles an overlap by — the two have to agree, which is why it is one method. It compared each diagnostic with
every one kept before it, O(d²) on the flood a long routine could raise.
**A "not found" names what was probably meant, but only when exactly one name is that near.**
`nearest_name` measures with the keyword-typo checker's own yardstick — `sql::edit_distance`,
one edit, two from seven characters on, case ignored, behind the same length pre-filter — and
its answer rides the diagnostic as the `suggestion` of `DiagMessage::TableNotFound`/
`ColumnNotFound`, which `DiagMessage::message` renders as the tail
`` — did you mean `employees`?``, or as nothing (the private `did_you_mean` that once spliced
the tail into a finished string at each site is gone, so the interface can word it — below).
All four not-found sites carry it:
`table_existence_checks`, from `Catalog::table_names` (the names a reference under that
qualifier could name, opaque synonyms included, **in the server's spelling** through
`Catalog::spelling` and quoted by `ident_if_needed` — the lower-cased key suggested `orderitems`
for PostgreSQL's `"OrderItems"`, which names no table; `a_suggested_table_keeps_the_servers_spelling`,
S8.1-L1-07, red before); `qualified_column_checks`, from the table's `columns_of`;
and both arms of `colres::resolve` — the qualified one from that source's known columns, the
unqualified one from every known column of the **innermost** scope's visible sources, which is
narrower than the chain the resolver walks before giving up. **It says nothing rather than
guess**: two *different* names equally near is `None`, naming one of them being a guess
presented as advice; the same name twice — two catalogue maps, or two sources in one scope,
holding it — is not a tie; and `name` itself among the candidates, in any case, is `None` too.
With nothing near, the message is exactly what it was
(`the_nearest_name_is_within_the_typo_distance_and_unique`,
`a_name_not_found_suggests_the_nearest_one`). What it costs is case: the catalogue keys tables
lower-cased and the resolver lower-cases a source's columns, so a mixed-case PostgreSQL table is
suggested in lower case, which is not the name to type; only the flat column check, reading
`columns_of` directly, suggests the server's own spelling.
**A diagnostic says what is wrong as a value, `Diagnostic::what`, not as a sentence.** It held
a finished `message: String`, which the squiggle's hover could only show in English once the
interface was translated; it is now a `DiagMessage`, a variant per thing a check reports whose
fields are the arguments — the names and words the statement spelt (`TableNotFound`'s `table`,
`database` and `suggestion`; `NullComparison`'s `fix`), counts (`InsertRowWidth`,
`RowWidthWithoutList`, `RowWidthOfTable`), and for a reserved alias the engine's `AliasQuote`
(backticks, double quotes, brackets) rather than the word for it. T-SQL's control-flow errors
are one variant, `Flow(FlowError)`, whose fifteen variants are what `TsqlFlow` reports through
`flow_error`, which takes one. Two variants carry text that is not Schemaic's to translate and
pass through as they came: `Syntax { detail }`, the parser's own English after the prefix
`friendly_syntax_message` strips (`None` when nothing is left), and `Server { text, shown }`,
the server's error as `clean_db_message` tidies it, built by `db_error_diagnostic` and
`run_error_diagnostic` through `DiagMessage::server` with `shown` empty. **`Server` carries the
error twice** because the server's words arrive inside the client's frame — `query failed: …`
— and the frame is Schemaic's: the hover showed it in English in every language. A caller
holding the interface wording (the app's `db_error_shown`, a `ModalError`'s `displayed()`)
sets it with `Diagnostic::shown_as`, which cuts it to its first line and strips MySQL/MariaDB's
*check the manual…* phrase exactly as the English is, and leaves any other variant unchanged;
`text` stays the English, which `locate_db_error` and `run_fix_range` still read
(`a_server_error_shows_its_wording_and_still_says_english`). `Diagnostic::message()` — `DiagMessage::message`, and
`FlowError::message` under it — is the English, byte for byte the text the field held, and it
is what the AI fix prompt reads (`problems_in_range`, above); the editor words `what` itself
(under `editor_pane.rs`), held to `message` in English by
`english_says_every_diagnostic_as_core_does`.
**A diagnostic with one right answer carries it as `Diagnostic::fix`**, Alt+Enter's quick fix
(the key is under `editor_pane.rs`). It is the text that replaces **exactly the diagnostic's own
`range`**, and has no range of its own on purpose: the per-statement cache shifts `range` when
an edit moves the statement, and a second span would have to be shifted everywhere the first
is. The not-found checks fill it with their own "did you mean" — an unknown table (its range is
the name token alone, so a qualifier stays) and an unknown column, qualified or bare, in the
per-scope resolver and the flat qualified check alike — quoted by `export::ident_if_needed`. A
column is written in **the catalogue's own case** (`Src::spelled`), not the folded key the
resolver matches by: quoting by case is only right once the case is the column's, and on
PostgreSQL `m."Ix"` was fixed to `m.id`, which names no column
(`a_column_fix_keeps_the_catalogues_case`). Three more checks fill it, and every other construction site states
`None`: `null_comparisons`
writes `x IS NULL` for `x = NULL` (`IS NOT NULL` for `<>`/`!=`) — nothing for `NULL = NULL`.
**It never rebuilds the operand from its AST span** (`null_fix`): sqlparser's spans leave out a
call's closing parenthesis, a cast's target and a subscript, and slicing by one turned
`ABS(salary) = NULL` into `ABS(salary IS NULL`. With the `NULL` on the right, everything before
the operator stays as written and only `op NULL` — the operator found over `skip_noncode`, the
`NULL` an exact one-token span — is replaced. With it on the left the operand has to move, which
is offered only when the rewritten statement parses back to `IS [NOT] NULL` over the same
operand, and withheld otherwise (`the_null_fix_keeps_an_operand_its_span_cuts_short`); the keyword-typo
check asks `nearest_name` over `SQL_KEYWORDS` and `STMT_KEYWORDS`, and the function-typo check
over the dialect's `CatalogIndex::lower`, both through the private `in_case_of`, which writes
the answer all lower or all upper when the typed word was, and as the name has it otherwise. So
a fix inherits `nearest_name`'s refusal to guess: a tie, or a typo the checker flags beyond the
yardstick's distance — `COUTN` is squiggled, but a transposition is two Levenshtein edits — is a
squiggle with no fix rather than a guess one key applies. `fix_at(diags, caret)` picks the
diagnostic: its range holds the caret **inclusively at both ends**, unlike the hover's
half-open `diag_under`, so a caret left just after the typo it finished still reaches it; and
among those *with a fix*, the strongest by `Severity::rank`, then the narrowest
(`a_fixable_diagnostic_carries_its_replacement`, `fix_at_finds_the_fix_under_the_caret`;
`a_diagnostic_with_no_single_answer_carries_no_fix` holds an unknown table and the `COUTN`
transposition to `None`; no test pins the tie).
**The false-positive corpus is only half a defence on its own, and that is why there are now two
passes over it.** `diag_bare` builds `Catalog::build(&[], None)`, so `unqualified_db_loaded` is
false, `table_status` answers `Unknown` for every unqualified reference, and
`table_existence_checks` and the column resolver emit **nothing at all** — the whole
unknown-table/unknown-column tier is switched off inside the test written to guard against false
positives. That is how `SELECT EXTRACT(YEAR FROM hired_at) FROM employees` could squiggle a red
``Table `hired_at` not found`` with the suite green: adding it to the old pass could not have
failed. So `valid_corpus` was lifted out of the test, and
`corpus_valid_queries_are_clean_against_a_loaded_catalog` re-runs its **catalog-true subset**
against `sample_catalog` with the tier on. A subset by necessity: a third of the entries name
`order`/`group`/`select` columns that catalog does not have, so there they are *correctly*
flagged, and running the whole list would assert the tier is off all over again. A filter that
stopped matching would let the test pass by running almost nothing, so it asserts a floor on how
many statements reached the catalog — the same failure mode `source_gate::crate_sources` guards
against for the UI's gates. The extra cases it carries (the FROM-separated calls, and two
subqueries as the control) run on **both** MySQL and PostgreSQL, since every test in that block
was MySQL and those bugs were on both.
**`FkEdge::table` is the declaring table's name in the case the server reported it**, and it
exists because the FK map is keyed on a *lower-cased* name so a reference can be looked up
however it was typed. `join_targets_for`'s reverse arm used that key for all three fields of the
suggestion it built, so on PostgreSQL a mixed-case table came back as `orderitems`,
`ident_if_needed` left an all-lower name unquoted (correctly, by its own rules), and accepting
the suggestion produced `relation "orderitems" does not exist`. Reproduced live on PG 16.15, and
equally broken on a MySQL server with `lower_case_table_names=0` — the Linux default, which is
why it is invisible on Windows. The forward arm was right for exactly the reason this one was
not: it takes the name from `FkEdge::ref_table`, stored in the server's own case, and this field
is that for the other end of the edge.
**`StarExpansion::columns` is carried rather than re-derived**: the completion row counted the
commas in the emitted SQL, which is the wrong quantity — a quoted identifier holding one
(`` `a,b` ``, legal on MySQL) over-reports — and the expander is the side that built the list.
**`expand_star` reads the star's own query**, not the statement: `star_query` bounds it from
its `SELECT` (the last at its depth in the innermost open paren, as `query_scope` finds it) to
the `)` that closes it, the next set-operation arm, or an upsert's `ON CONFLICT`/`ON DUPLICATE
KEY`, and `statement_scope` is asked of that slice alone. Over the whole statement the list
gained an `INSERT`'s target, the other `UNION` arm's tables, and — once `statement_scope`
appended them for `new.` to complete — a trigger's `NEW`/`OLD` and an upsert's `excluded`,
none of which the `SELECT` can name (*Unknown table 'new'*). Those rows are read off a
statement head the slice has not got, so it has none. An unqualified star over a `USING` or
`NATURAL` join is declined: `*` returns the merged column once — coalesced, under an outer
join — so a list of each side's would add a column and change its values; `t.*` is `t`'s own
and still expands. So is one over a CTE, which is not the catalogue table it may be named like
(`a_star_expands_to_its_own_querys_tables_only`,
`a_star_over_a_using_or_natural_join_is_not_expanded`,
`a_star_over_a_cte_named_like_a_table_is_not_expanded`, all red before).
**`simple_select_source` is the one definition of "structurally simple enough to aim a write
at"** — one statement, a `SELECT` body, no CTE, one `FROM` entry, no joins, a plain named table
— shared by `filter::build_query` (which needs to know it may splice a `WHERE` into the
statement that produced a result), by `filter::page_size`/`with_page` (so a statement the pager
turns is one the filter bar could also rewrite) and by SQLite's write-back, which reaches it through
`projection_of` and — now that the driver's own provenance does the attributing — needs it only
to recover the *spelling* a projected `rowid` was written with. Two predicates that agreed on the day they were written is the
arrangement `ident_sql` exists to rule out, and a test pins that the two callers still agree.
`single_source_table` is its entry point for a caller holding only SQL.
**`full_table_source` is the stricter twin of it**, and the two are not
interchangeable: `single_source_table` asks which table a row came *from*, while this asks
whether the statement returns the table **entire** — no `WHERE`, no `GROUP BY`/`HAVING`/
`QUALIFY`, no `DISTINCT`, no `LIMIT`/`OFFSET`/`FETCH`, and a projection of nothing but column
references and wildcards. Only then is a table's row estimate also an estimate of what the
*query* would have returned, which is what the results toolbar's `1,000 of ~4.2m` claims. The
projection rule is there for aggregates — `SELECT count(*) FROM t` passes every other test and
returns one row — and refuses anything computed without looking closer, arithmetic included:
being wrong invents a total, being conservative only says less. An `ORDER BY` passes, so the
difference between the app's own generated browse query and a qualifying one is precisely its
`LIMIT`, which a test pins. It also looks **inside the table factor**, not only at the name it
carries: `FROM orders PARTITION (p0)` and `TABLESAMPLE SYSTEM (10)` each read a proper subset, so
the table's estimate is a total the statement could never have reached — and the `~` in front of
it says "estimate", not "of a different query". A lock hint (`FOR UPDATE`) and
`SQL_CALC_FOUND_ROWS` sit in the same neighbourhood, change no row count, and still qualify;
a test pins that too.
**`provenance_sources` is the gate on a *driver's* per-column provenance**, and `db::sqlite`'s
`attach_origins` is its only caller. It answers `None` for what it cannot read — a statement that
doesn't parse, several statements, anything that isn't a `Query` — and for a **set operation at
any depth**, one buried in a derived table or a CTE body included; otherwise it hands back every
base relation the statement reads, with CTE-bound names filtered out, since those shadow any real
object of the same name and the body they stand for was walked here already.
**The shadowing is scoped, and it is a stack rather than a set** (`Sources { relations, scopes }`
— `pre_visit_query` pushes a frame of that query's own CTE names, `post_visit_query` pops it, and
a relation is decided *while* its enclosing frames are on the stack, since afterwards nothing
tells an outer relation from an inner one). It was one flat list of every CTE name met at any
depth, applied to every relation after the walk, so a **nested** `WITH` stripped a real object of
the same name out of the enclosing scope: `SELECT a.id, v.x FROM a JOIN v ON v.id = a.id CROSS
JOIN (WITH v AS (SELECT 1 AS z) SELECT z FROM v) q` came back `[a]`, with the real view `v` gone.
On SQLite that is the whole failure — the caller never learns `v` is a view, provenance reports
one branch's table for a compound view, so `v.x` is attributed to `main.a` and offered as
editable, and the rows that came from `b` are edited against `a`. The 1-row net catches the
usual outcome but is the backstop and not the gate, and it does not catch an `a` holding a row
with the same key. A *top-level* `WITH` cannot produce this, the name being shadowed
statement-wide, which is why the flat rule read as safe. Three tests hold the scoping apart:
`a_nested_cte_does_not_shadow_a_name_in_the_outer_scope` (in both orders, the nested query first
being the one that needs the frame *popped* rather than merely pushed),
`a_top_level_cte_still_shadows_its_own_name` and
`a_nested_cte_still_shadows_inside_its_own_query` — the last because the fix must not simply stop
shadowing. The set-operation
refusal is the load-bearing part, and it is there because provenance answers a narrower question
than its name suggests: it names the table a result *column expression* resolves to, not the table
a result *row* came from. Measured on SQLite 3.53.2,
`SELECT id, name FROM t UNION SELECT id, note FROM u` reports `t` for the whole result, while the
same union read through a view reports `u` — so it is not a consistent rule a caller could
compensate for, it is whatever the code generator resolved last. Half the rows are in the other
table, so attributing by it would `UPDATE` a table the row was never in. **The relations come back
so the caller can refuse the ones this crate cannot check**: a compound hidden behind a *view*
leaves nothing in the statement to see it by — `SELECT * FROM v` parses as a plain single-table
read whatever `v` is — and only a catalogue can say which of these names is a view, which is a
question a pure module has no connection to answer. Three tests hold it:
`provenance_sources_names_every_table_a_readable_shape_touches`,
`a_set_operation_at_any_depth_refuses_provenance` and
`provenance_sources_refuses_what_it_cannot_read`.
**`projection_of`** is the derivation over `single_source_table`: which base column each result column reads, or
`None` where it is computed. It is **positional, not name-matched**, and that is the whole
reason it is a function rather than a lookup — `SELECT a AS b, b FROM t` produces a first
column *named* `b` that *is* `a`, so matching by name maps it to column `b` and an edit to that
cell would `UPDATE` the wrong column silently, with the grid showing the change as though it had
worked. Reading the projection positionally gets both right, and gets an alias right for the
same reason MySQL's `org_name` does: an alias renames the output, not the column behind it. A
`*` mixed with anything else is resolved only as far as it safely can be: no position *after* a
wildcard is knowable, since its width isn't, and a provenance list off by one column is the
worst possible answer — so a `*` with anything behind it, or a second `*`, still answers `None`
overall. Positions *ahead* of a lone trailing wildcard were refused by that same rule for a
reason that never applied to them, because nothing the wildcard expands to can shift them.
They are placed now, as `Projection::LeadingThenWildcard(items)` — the leading items in order,
with the caller appending the source table's own columns exactly as it does for
`Projection::Wildcard`. That is what makes `SELECT rowid, * FROM t` analysable and so a keyless
SQLite table editable, but the relaxation is **general**: `intel` carries no SQLite vocabulary
and doesn't know why anyone wanted it. Two tests hold either side of the line —
`projection_places_the_columns_ahead_of_a_trailing_wildcard` and
`projection_still_refuses_a_wildcard_that_is_not_the_last_item`.
**The relaxation moved the safety into another crate**, which is why it is tested from both
ends. `SELECT a, * FROM t` used to return `None`, and no projection meant no origins meant
read-only by construction; now every column is attributed and two of them claim the base column
`a`, so the only thing still refusing the table is `edit::resolve_key`'s C1 duplicate check.
`db::sqlite`'s `a_column_exposed_twice_by_the_widened_projection_stays_read_only` walks the new
shape through `analyze_edit` end to end, `a_computed_leading_item_keys_on_the_tables_own_primary_key`
covers the other half (`SELECT 1, * FROM t`, editable only *because* of the relaxation), and
`edit::c1_holds_for_one_column_duplicated_within_a_single_table` names C1 explicitly so a
relaxation of the duplicate rule fails there rather than in a SQLite integration test.
**Every SQL fragment this module generates** (`join_condition`, `join_targets`, `expand_star`)
is quoted through `export::ident_if_needed`, which quotes only what a bare name would get wrong
— anything that isn't a plain ASCII word that can stand unquoted as an *identifier*
(`must_quote_ident`, below). PostgreSQL folds an unquoted
`ArtistId` to `artistid`, so unquoted output couldn't run on any mixed-case schema, while
unconditional quoting would backtick every ordinary MySQL name in text the user is about to
edit. **A capital letter is a reason to quote only where the engine folds**
(`SqlDialect::folds_unquoted_names`, PostgreSQL alone): MySQL keeps a bare name's case and
SQLite and SQL Server match it whatever its case, so quoting for case there bracketed every
PascalCase SQL Server name the editor generated
(`ident_if_needed_quotes_exactly_what_a_bare_name_would_get_wrong`). `JoinTarget` therefore carries the **bare** name for the popup to display and prefix-match
and `table_sql` for what is actually inserted.
**A reserved word is two questions, and they carry opposite costs.** `is_reserved_word` (over
`MYSQL_RESERVED` / `PG_RESERVED` / `SQLITE_RESERVED`) asks whether a word can be a bare
**alias**, and backs the nine diagnostic sites here — the alias check, the scope's alias
resolution, the botched-alias warning — where listing a word wrongly squiggles working SQL, so
those lists deliberately lean towards missing one. `must_quote_ident` asks whether it can be a
bare **identifier**, a table or column name, which is the question a quoter has: miss a word
there and the SQL emitted does not parse. On MySQL and PostgreSQL the two coincide, so
`alias_ok_but_unquotable` is empty for both — a reserved word is reserved everywhere and one
list answers both. (SQL Server's holds the six documented words its parser accepts, above.) SQLite is where they come apart, because its parser falls back to treating
most of its ~147 keywords as identifiers wherever the grammar allows one: `CAST`, `IF` and
`RAISE` are refused as a bare name yet are perfectly good `AS` aliases. Until the two were
split, `export::ident_if_needed` and `filter::needs_quoting` were asking the **alias** set, so
`filter::table_query` over a table `if` keyed on a `cast` column generated
`SELECT * FROM if ORDER BY cast ASC LIMIT 10` and SQLite answered `near "ASC": syntax error`.
**The MySQL family is two servers here, and the two questions take them differently.**
`MYSQL_RESERVED` is MySQL 8.4's, matching 8.4.11's `information_schema.KEYWORDS` where
`RESERVED = 1` word for word (`mysql_reserves_what_8_4_reserves`) — it was 8.0's, which still
held `MASTER_BIND` and `MASTER_SSL_VERIFY_SERVER_CERT` (8.4 takes both as aliases) and lacked
`QUALIFY` and `TABLESAMPLE` (8.4 refuses both). The cost is on 8.0, past its end of life: an
`AS qualify` it accepts is now warned about. MariaDB, measured word by word against its own
`information_schema.KEYWORDS` in all three alias positions on 10.11.14 and 11.8.9, takes the
thirty-two words of `MARIADB_FREES` as aliases (the window functions, `LATERAL`, `ROW`,
`QUALIFY`, …) and refuses the fifteen of `MARIADB_ADDS` (`OFFSET`, `RETURNING`,
`CURRENT_ROLE`, …). A word only **one version** reserves sits in a third list,
`RESERVED_BY_SOME_VERSION` — 8.0's two above; `MASTER_SSL_VERIFY_SERVER_CERT` again, which
10.11 refuses and 11.8 takes; and `VECTOR`, `SQL_AFTER_GTIDS` and `SQL_BEFORE_GTIDS`, which
11.8 refuses and 10.11 takes. The server's version is not known, so the warning never reads
it — a false squiggle is the worse mistake there — and only the quoter does. Before it
existed the 11.8 words were left out of both, so a column named `vector` went into generated
SQL bare and would not parse on 11.8. The **alias check** asks `is_reserved_word_on` with the
server's flavour,
which `Catalog` now carries (`Catalog::flavour`, from the active database's `DbSchema`), and
with none known holds to what both reserve — the warning's side
(`mariadb_reserves_its_own_words`, `the_alias_check_follows_the_servers_flavour`). Its implicit
`orders offset` form is read by the table-list walk, which has no flavour: the walk decides
where a table reference ends by what **every** server of the dialect reserves
(`reserved_on_every_server` — `is_reserved_word_on` with `ServerFlavour::Unknown`), and hands
`alias_checks` any implicit alias it took that *some* server reserves, for the flavour to
judge. So MariaDB's `orders current_role` and `orders offset` are flagged there, and MySQL 8's
`orders rank` on MySQL 8 alone (`a_word_mariadb_adds_is_flagged_as_an_implicit_alias_there`).
The walk used to read MySQL's list, which left a MariaDB-only word unflagged — and broke the
scope the other way: `FROM employees rank` registered no alias, so on MariaDB, which frees
`rank`, mid-edit `rank.` completed nothing and `rank.id` was an unknown qualifier
(`a_word_mariadb_frees_is_an_alias_in_scope`). The trade is MySQL 8, where `rank` is now read
as the alias it refuses — the squiggle on it is the alias check's. The **quoter** cannot know
the server, so `must_quote_ident` takes both, and every version (`reserved_anywhere`): a MariaDB column named
`offset` had gone into generated SQL bare (`a_word_either_mysql_family_server_reserves_is_quoted`,
`a_word_one_server_version_reserves_is_quoted`). The other
`is_reserved_word` sites — `reserved_in_alias_slot` and `joined_table` among them — still read
MySQL's list.
Neither list is a transcription of anybody's documentation any more: `db::sqlite`'s
`the_reserved_lists_match_what_sqlite_itself_refuses` walks the engine's own keyword table
(`sqlite3_keyword_count`/`sqlite3_keyword_name`) and compiles every keyword on a real in-memory
connection in three positions — bare column name, bare table name, bare `AS` alias — asserting
both predicates against what SQLite actually accepts. It is a standing guard rather than a
snapshot, so a keyword a future release adds arrives on its own and fails. Against 3.46.0's 147
keywords — the amalgamation bundled when it was written — it found the 57 existing
`SQLITE_RESERVED` entries all correct and exactly four words missing from the identifier set:
the three above, plus `NOTHING` (from `ON CONFLICT DO NOTHING`), which is refused in every
position and so joined `SQLITE_RESERVED` itself. The bundled engine has since moved to
**3.53.2** (`rusqlite` 0.32 → 0.40, carrying `libsqlite3-sys` 0.30 → 0.38) and the guard passed
that bump unchanged, still over 147 keywords. Nobody re-read SQLite's keyword list to find that
out, which is the whole reason this is a test and not a comment.
**Which position a word breaks in is load-bearing**: `CAST` and `RAISE` are refused as a bare
*column* name but accepted as a table's, and `IF` is the reverse — a first draft of the
end-to-end `a_table_named_for_a_keyword_still_opens` put each one on the side it tolerates and
passed with the fix reverted. `a_word_can_need_quoting_as_a_name_yet_be_a_fine_alias` pins the
three-word gap so the two lists can't be tidied back into one. Existing data saw no change:
across the Chinook, Northwind and EdgeCases files no identifier changed quoting status and no
generated statement failed, so this closed a latent bug rather than altering behaviour.
The live DB stays the semantic authority: **Tier-2 live validation**
PREPAREs the statement under the cursor (`Db::prepare_check`, non-executing) behind the
`live_validate` setting (off by default), merging dialect-exact errors into the editor squiggles.
Those squiggles are one server's verdict, so moving the tab to another connection, database or
engine clears them and bumps the validation generation, dropping a round-trip still in flight
to the old one (an effect in `editor_pane`, beside the one that clears them when validation is
switched off; no unit test).
**`sql_reply` is a gate on what a model may put in the editor, not a cleanup.** Ctrl+K's output
goes straight into the user's buffer, and while the model is told to answer with bare SQL the
*tool* it runs in can still put a line of its own on stdout: a `claude -p` run returned an MCP
diagnostic (`Client.listTools() called but server does not advertise tools capability…`)
stitched onto a perfectly good statement, and Ctrl+K offered the pair as an edit. So what cannot
be parsed is not offered. The whole reply is tried first — anything that already parses comes
back untouched — and only then does it try dropping up to `REPLY_TRIM_LINES` (3) lines from each
end, fewest first, never from the middle. The load-bearing detail is that **a line may only be
dropped if it cannot be SQL**: its first word is not a keyword (`is_sql_keyword`), and a line
opening with punctuation (`);`) is protected too. Without that guard, trimming the ends
*truncates* — given `SELECT a` / noise / `FROM t`, dropping the last two lines leaves
`SELECT a`, which parses perfectly and means something else entirely. That is what
`noise_in_the_middle_is_not_stitched_around` pins, and it is the reason the guard exists. The
cost is deliberate: a valid statement this parser cannot handle is refused rather than shown.
**`predict` is the editor's inline ghost — deterministic, not AI, and offered only where the
answer is close to certain.** It returns the text Tab would insert at the caret, from ten
producers asked in order, each answering only its own shape: an empty `JOIN … ON` gets the
foreign-key predicate (`on_condition_guess`, below); `INSERT`/`REPLACE … INTO t (` (its target read as the
engine writes it, below) gets the columns an `INSERT` writes, from `Catalog::insert_columns_of`, quoted through
`export::ident_if_needed` like every fragment above, plus a `)` unless one already follows
(`column_list`); a `(` whose contents the schema fixes — `REFERENCES t (`, `ON CONFLICT (`,
`USING (` — gets them (`paren_contents`, below); `ON DUPLICATE KEY UPDATE ` and
`ON CONFLICT (k) DO UPDATE SET ` get the update list (`upsert_assignments`, below);
`GROUP BY ` gets the grouping list; an empty `JOIN ` gets the whole join, `table ON condition`
(`join_target_guess`); `JOIN t alias ` gets `ON` and the condition (`join_on_guess`); a
keyword whose partner the grammar fixes gets the partner (`ORDER`/`GROUP`/`PARTITION` → `BY`,
`PRIMARY`/`FOREIGN` → `KEY`, `INSERT` → `INTO`, `DELETE` → `FROM`, the join words → `JOIN`,
`IF` → `[NOT] EXISTS`, a column's `NOT` → `NULL`, and a handful of pairs such as
`IS [NOT] DISTINCT` → `FROM`, `ON DUPLICATE` → `KEY UPDATE` and a hand-typed
`ON DUPLICATE KEY` → `UPDATE` — `keyword_partner` is the list),
in lower case when the keyword was typed all-lower — `in_typed_case`, which writes both join
producers' `ON` in the case `JOIN` was typed; an operand just written gets the
word its construct fixes next (`operand_partner`, below); and, asked last, a written construct
gets the word it cannot do without (`clause_partner`, below). It answers nothing unless the rest of the
caret's line is whitespace and `)` — the editor auto-pairs a `(`, so `INSERT INTO t (|)` has a
closer nobody wrote — and at most the statement's own closing `;`, which going back into a
finished statement always leaves after the caret (`ORDER |;`), though a second statement after
it on the line still refuses (`a_statements_own_semicolon_after_the_caret_still_predicts`), or
is the next clause, apart from the caret (below); the
character before the caret is whitespace or `(`; and the word it would offer is not the one the
statement already has next (editing `ORDER |` above an existing `BY` offers no second one).
That word is compared whole: a non-ASCII character never ends it, where casting each `char` to
a byte for `is_word_byte` cut `名前` at its first character and offered it again
(`a_non_ascii_word_already_next_is_not_offered_again`). The same reading of a word decides how
Ctrl+→ takes a ghost one word at a time: `prediction_word_end` is the byte length one press
takes — any punctuation leading in, whitespace, the word's own characters and the whitespace
after them — so `id, name)` goes `id` | `, name` | `)` and `customers ON o.customer_id` goes
`customers ` | `ON ` | `o` | `.customer_id`. A non-ASCII character is a word character there
too, as `is_word_byte` has it, and the answer is never 0 for a non-empty text, so every press
takes something (`a_prediction_is_taken_a_word_at_a_time`; the key is under `editor_pane.rs`).
It takes the editor's `SqlDialect`, for two reasons. A quoted name or a string is one word,
read through `skip_noncode` in that dialect — `"order details".id` goes `"order details"` |
`.id`, `[order details] ON ` goes `[order details] ` | `ON ` — since a press that stopped
inside the quotes would leave the buffer holding half a name, and every keystroke after it
inside an open quote. And a word's characters include `sql::continues_name`'s, so `$`
everywhere and T-SQL's `@`/`#` keep a name whole: `@total + 1` goes `@total ` | `+ 1`
(`a_press_takes_a_quoted_name_whole`).
It also refuses a caret in a string, comment or quoted name, and
that is `caret_touches_noncode` walking only the text *before* the caret through `skip_noncode`,
so an unterminated quote or a line comment running up to the caret counts as the caret being
inside one — the one boundary lexer, not a new scanner. **Nor does it answer past the
statement's end** — after the `;` that closes it, on the rest of that line or the blank lines
below — which `sql::statement_range_ended` reports, since `statement_range` there answers the
statement *before* the caret: a caret on the line after `DELETE FROM t;`, or after a space
following its `;`, read `DELETE FROM t` and ghosted `WHERE `, which Tab wrote after the `;`
(`a_caret_past_a_finished_statement_predicts_nothing`). Back in front of that `;`
(`DELETE FROM t |;`) it still predicts.
**Mid-line, a ghost goes only in front of the next clause.** `starts_a_clause` wants whitespace
straight after the caret, then a whole word from `CLAUSE_OPENERS` (`FROM`, `WHERE`, `ORDER`,
`LIMIT`, the join words, `ON`, `USING`, `SET`, `VALUES`, `INTO`, …): `ORDER | LIMIT 10` is two
clauses apart and `BY` belongs between them. Glued to the caret (`ORDER |LIMIT`) or before
anything else — an operand, a list item, `BETWEEN 1 | 5`, `INSERT INTO t (| id)` — the text
after the caret is part of what is being written, and a prediction would split it; so
`ORDER | id` still predicts nothing (`a_prediction_goes_in_front_of_the_next_clause_on_the_line`,
red first; `nothing_is_predicted_mid_line_or_for_a_word_already_there`). There its trailing
whitespace is trimmed, since the space already after the caret stands in for it (`BY`, not
`BY `). The auto-paired `)` counts only at a line's end (`closed` is read only there), so a
caret against one mid-line, as in `INSERT INTO t (|) VALUES (1)`, gets no ghost. **Nor does
one go in a line's leading indentation**: with nothing but whitespace before the caret on its
line and the line's code after it — `JOIN\n  |  WHERE …`, `ORDER\n| LIMIT 10` — the caret is
re-indenting that clause, not writing in front of it, and a ghost would split the clause off
its indent. A blank indented line, where the statement goes on, still predicts
(`nothing_is_predicted_in_a_lines_leading_indentation`).
**A line's end is not the statement's: whatever follows the caret, on this line or the ones
below, is what the ghost is written in front of**, and one rule holds it in both places. The
first word after the caret up to the statement's end (`statement_range`, so its `;`) must open a
clause or a statement — `opens_a_clause_or_statement`, `CLAUSE_OPENERS` or `STATEMENT_OPENERS`,
the second because with no `;` before it the editor reads the next statement as part of this
one, so `ORDER |\n\nSELECT 1` still gets `BY `. The prediction may not hold that word
*anywhere*, not only first — `JOIN | ON o.x = c.y` would otherwise gain `customers ON …` and a
second `ON` (`mid_line_predicts_nothing_that_would_collide_with_what_follows`). And
`may_precede(guess, next)` must let it go there: a guess holding `WHERE` only before a clause
that may follow a condition (`AFTER_WHERE` — `GROUP`, `ORDER`, `HAVING`, `LIMIT`, `OFFSET`,
`FETCH`, `WINDOW`, the set operations, `RETURNING`), one holding `ON` only before those or
`AFTER_ON` (`WHERE`, the join words, MySQL's `UPDATE … JOIN … SET`), one holding `FROM` never
before `INTO`, and anything before a statement opener. Two holes made this one rule. At a
line's end only the next word was compared, and only with the guess's first, so the line break
hid the rest of the statement: `JOIN |\n  ON o.customer_id = c.id` predicted `customers ON …`
and a second `ON`, `JOIN |\n  customers c ON …` another table, `GROUP BY |\n  total` a second
list, `INSERT INTO customers (|\n  name)` a list clashing with the one written,
`LEFT |\n  OUTER JOIN` a `JOIN`, `ON |\n  c.id = o.customer_id` a duplicate condition and
`IF |\n  EXISTS` a `NOT EXISTS` (`a_prediction_reads_what_the_statement_says_on_the_lines_below`,
red first). Mid-line, the check kept the next clause's word out of the guess but not the guess
out from in front of a clause it may not precede: `JOIN | USING (id)` got an `ON`,
PostgreSQL's `DELETE FROM t | USING …` a `WHERE` before the `USING`, T-SQL's
`DELETE FROM t | FROM …` one before the second `FROM`, and `SELECT * | INTO #t FROM t` a `FROM`
(`a_prediction_goes_only_before_a_clause_it_may_precede`, red first). The one exception is a
caret in a closed paren — `closed`, the auto-paired `)` — whose contents end at the `)`
whatever follows it, so `INSERT INTO customers (|)\nVALUES (1, 'a')` still gets its columns.
The cost is a statement that goes on below with anything else: editing the top of an
unterminated statement whose next line is an operand or a list item gets no ghost, and nor does
a statement left without its `;` above one whose first word `STATEMENT_OPENERS` does not list
(`PRAGMA`, `COPY`, T-SQL's `PRINT`).
Each keyword guard is a place the bare
rule was wrong: `INSERT`/`DELETE` count only as a statement's first word, since both also sit
in a trigger header and a key's `ON DELETE`; `GROUP` takes `BY` only where it groups a
query's rows (`groups_rows`) — its own query, from `local_scope_start`, has a `SELECT` at its
depth, and the word before it is none of `WITHIN`, `TO`, `FROM`, `CREATE`, `ALTER`, `DROP`,
`RESOURCE` or `AVAILABILITY` — where the bare rule offered `BY` after PostgreSQL's
`CREATE GROUP` and `GRANT … TO GROUP`, MySQL's `CREATE RESOURCE GROUP` and T-SQL's
`ALTER AVAILABILITY GROUP`; `WITHIN GROUP (`, once a guard of its own, is one of those words.
`PARTITION` takes it only where a partitioning is declared (`declares_a_partitioning`):
straight after a window's own paren (`OVER (PARTITION`, `WINDOW w AS (PARTITION`), or after a
`)` that returns a `CREATE … TABLE` to depth 0, the one closing its column list. It used to
answer everywhere — MySQL's `FROM t PARTITION (p0)` and
`ALTER TABLE … ADD`/`DROP`/`TRUNCATE PARTITION`, PostgreSQL's `PARTITION OF` and
`ATTACH PARTITION`, T-SQL's `CREATE PARTITION FUNCTION`, a column named `partition`. The
narrower rule's cost is a partitioning declared
anywhere else: MySQL's `ALTER TABLE t PARTITION BY`, and a `CREATE TABLE` with table options
between its `)` and the `PARTITION`, get no ghost
(`partition_and_group_predict_by_only_where_they_partition_or_group`). `PRIMARY`/`FOREIGN`
take `KEY` except after `CREATE`, `ALTER`, `DROP`, `IMPORT` or `ON`, where the word names an
object — PostgreSQL's `CREATE FOREIGN TABLE`, `IMPORT FOREIGN SCHEMA` and
`ON FOREIGN DATA WRAPPER`, T-SQL's `CREATE PRIMARY XML INDEX` — and anywhere in a statement
where it can only name one: `CREATE SERVER s FOREIGN DATA WRAPPER` (PostgreSQL and MySQL) and
`ALTER EXTENSION e ADD FOREIGN TABLE`. Those are read off the statement's first two words, not
the word before, which a `CREATE SERVER`'s `TYPE 'x' VERSION '1'` can stand between
(`primary_and_foreign_predict_key_only_in_a_definition`). `ORDER` takes `BY` except in a
`CREATE … COLUMNSTORE INDEX` — `COLUMNSTORE` as the statement's second or third word, so
`CLUSTERED`/`NONCLUSTERED` may come between — whose T-SQL ordered form takes `ORDER (col)`
with no `BY` (`a_columnstore_index_order_predicts_no_by`). A join word
predicts `JOIN` only where a join can go, since `LEFT`/`RIGHT` are string functions too —
`in_join_position` reads the word's own query from `local_scope_start` through `scan_clauses`
and wants a `FROM`, join, `ON` or `USING` clause, or an `UPDATE`'s table list where
`update_takes_joins` says one may (MySQL's multi-table `UPDATE a INNER JOIN …` alone; the
others join in an `UPDATE … FROM`, which reads as a `FROM`), where waiting for any earlier `FROM` in the statement
predicted `JOIN` after `WHERE LEFT ` and `ORDER BY LEFT `, and in a subquery's projection off
the outer query's `FROM` (`join_words_predict_only_where_a_join_can_go`). Inside an `ON` it
also wants the condition so far to be a finished operand (`operand_finished`, below): the
clause alone said yes to `ON LEFT ` and `ON c.name = RIGHT `, predicting `JOIN` where the user
is writing the `LEFT()`/`RIGHT()` functions, while `ON o.customer_id = c.id LEFT ` still gets
it (`a_join_word_in_an_unfinished_condition_predicts_nothing`). `FULL` predicts it only where
`has_full_joins` says the engine has the join — not MySQL or MariaDB, which read
`FROM a FULL JOIN b` as `FULL` being `a`'s alias
(`full_predicts_join_only_where_the_engine_has_one`). `CROSS` predicts nothing where
`has_apply_joins` says an `APPLY` may follow — T-SQL alone. `OUTER` predicts `JOIN` only straight
after `LEFT`, `RIGHT` or (where `has_full_joins` says so) `FULL`: there no `APPLY` can follow,
and a bare `OUTER JOIN` is no join on any engine. The `APPLY` guard used to refuse every `OUTER`
on SQL Server, `LEFT OUTER ` included, while a bare `OUTER ` was offered `JOIN` everywhere else
and MySQL's `FULL OUTER ` slipped past `has_full_joins`
(`an_outer_join_predicts_join_on_sql_server_too`,
`full_predicts_join_only_where_the_engine_has_one`). Both are asked as a capability through an
exhaustive match, so a new engine has to answer them. `REPLACE`/`MERGE`
(→ `INTO`) and `TRUNCATE` (→ `TABLE`) count only first, as `INSERT` does (`SELECT REPLACE `
predicts nothing). Some of those words are optional on some engines (`INTO` after `INSERT` on
MySQL and SQL Server, `TABLE` after `TRUNCATE` everywhere but T-SQL); the partner is then the
conventional spelling every engine that has the statement accepts, not a guess. `IF` is also a T-SQL statement, a procedure body's branch and MySQL's
`IF()`, so `if_partner` answers only with an object word (`DDL_OBJECT_WORDS` — `TABLE`,
`VIEW`, `COLUMN`, …) right before it, and the nearest verb behind that decides: `DROP`/`ALTER`
→ `EXISTS`, `CREATE`/`ADD` → `NOT EXISTS`; `IF NOT` gets `EXISTS` only after an `IF` that
qualified (`if_predicts_exists_by_the_verb_it_belongs_to`, `if_elsewhere_predicts_nothing`).
`ALTER` came late: the look-back knew only `DROP`, `CREATE` and `ADD`, so PostgreSQL's and
MariaDB's `ALTER TABLE IF ` got no `EXISTS`. *Nearest* is what keeps it safe to add — an
`ALTER`'s `ADD COLUMN IF ` and `DROP COLUMN IF ` still find their own verb first, and get
`NOT EXISTS` and `EXISTS` (`the_words_as_certain_as_the_others_are_predicted_too`, red first
on that `ALTER TABLE IF `).
`NOT` negates anything in a condition, so it gets `NULL` only where `declares_a_column` finds
a column being defined — a `CREATE … TABLE`'s column list (one paren deep) or an
`ALTER … TABLE`'s clause (none) — and so never inside a `CHECK (` or a
`CREATE TABLE … AS SELECT … WHERE` (`not_in_a_condition_predicts_nothing`). The item it sits
in, since the last comma at that depth, must also hold none of `WHERE`, `CONSTRAINT`, `CHECK`,
`FOREIGN` or `REFERENCES`: a constraint's `NOT` is PostgreSQL's `NOT VALID` or
`NOT DEFERRABLE`, and the first cut, without that rule, offered `NULL` there
(`not_after_a_constraint_predicts_nothing`). Its cost is a column with an inline `REFERENCES`,
whose `NOT NULL` gets no ghost. A table constraint `CONSTRAINT` does not name carries none of
those words, so the item's opening word is read too — the word after the comma or the `(`
that opens the list, or after an `ALTER … TABLE`'s `ADD` — and `PRIMARY`, `UNIQUE`, `EXCLUDE`, `KEY` or `INDEX` there makes it
a constraint, whose `NOT` is `NOT DEFERRABLE`: without it `UNIQUE (a, b) NOT `,
`PRIMARY KEY (a) NOT `, an `EXCLUDE … NOT ` and `ADD PRIMARY KEY (a) NOT ` were all offered
`NULL`. A column's own `a int UNIQUE NOT `, whose kind comes after its name, still is
(`not_after_a_table_constraint_predicts_nothing`).
`CREATE UNIQUE` and `CREATE OR` are asked as capabilities the
same exhaustive way: `unique_index_names_its_kind` makes `INDEX` a guess on T-SQL alone
(`CREATE UNIQUE CLUSTERED INDEX`), and `create_or_spelling` answers `REPLACE` on MySQL and
PostgreSQL, `ALTER` on SQL Server and nothing on SQLite, which has neither form
(`create_modifiers_are_answered_per_engine`). The `INSERT` target
is read as each engine writes it: `t` and `q.t`, and T-SQL's `db.schema.t` and `db..t`, resolved
through `Catalog::resolve_located` as the checker's three-part names are; and with no `INTO` at
all (`INSERT t (`) where `insert_into_is_optional` says the grammar allows it — MySQL and SQL
Server, PostgreSQL and SQLite requiring the word — asked the same way. It used to want `INTO`
and at most one qualifier, so a ghost the engine's own spelling should have had never came
(`an_insert_target_is_read_as_the_engine_writes_it`).
**The column list is `skeleton::skeleton_columns`'s, not the table's.** Listing every column
named the key the server numbers — a statement SQL Server refuses outright without
`IDENTITY_INSERT` — and any computed column, which every engine refuses; and a ghost that
disagreed with the Generate menu's `INSERT` draft about what an `INSERT` writes would be two
answers to one question. `Catalog` stores only column names, so `Catalog::build` asks
`skeleton_columns` of each table and records the columns it leaves out in `insert_skips`, keyed
under the database, `""` for a bare in-scope reference and the PG/T-SQL namespace — the same
three keyings as the column maps, so like them two same-named tables under one key share their
skips (where the two have the same columns, an identity `id` on `sales.orders` drops `id` from a
bare `orders (` meant for `dbo.orders` too: a column left out, which typing restores, never one
wrongly named) — and `insert_columns_of` is `columns_of` less those. **Where their columns
differ it answers nothing at all** (`Catalog::is_union`, under the synonym paragraph above):
the bare map holds both tables' columns, so PostgreSQL's `INSERT INTO orders (` predicted
`id, customer_id, code)`, a list neither table has. `is_union` reads the entry the reference
resolves to — the `(database, name)` one when its qualifier is a database holding the table,
the bare one for a bare name — and never a namespace-qualified reference, which reads that one
table's own `schema_qualified` entry; so `INSERT INTO sales.orders (` still predicts
`id, customer_id)` (`a_name_two_tables_share_predicts_nothing_from_their_union`, red before on
exactly that `INSERT`). The
skeleton's fallback comes with it, so a table that is only a numbered key still names that key
(`an_insert_column_list_leaves_out_what_the_server_assigns`,
`an_insert_into_a_key_only_table_still_names_the_key`).
**The grouping list is read off the parse, not the text** — the rule this module exists to
hold. The query up to `GROUP` is a whole query on its own, so it is parsed with sqlparser and
its grouping keys listed through the AST's `Display`, which is why `x AS c` gives back `x`, not
`c`. *Which* query is the one the `GROUP BY` belongs to: the text after the innermost `(` still
open before `GROUP` (`local_scope_start` — a derived table, a CTE body), else the statement;
within that, the `SELECT` an `INSERT`, a `CREATE VIEW` or a `CREATE TABLE … AS` is built
from, and a set operation's
last arm, the one being typed (`group_by_reads_the_query_it_belongs_to`). It used to parse the
whole statement, so a `GROUP BY` inside a subquery predicted nothing at all. **An item is a key
only if it reads a column** and holds no aggregate, window or subquery: a constant or a
column-less expression groups nothing, and the engines refuse it — PostgreSQL a non-integer
constant (`GROUP BY 'x'`), SQL Server any expression without a column, and a subquery there too
(`group_by_leaves_out_constants_and_subqueries`). A variable is no column: T-SQL's `@region`
reaches the AST as an identifier, and SQL Server refuses `GROUP BY @region` (Msg 164), so a
name beginning `@` does not count as a read. **An aggregate or window item is no key, but what
it reads outside a plain aggregate's arguments still has to be grouped** — `total * count(*)`,
or `sum(total) OVER ()`, whose window runs over the grouped rows — and when the projection's
own keys do not already cover such a column nothing is predicted, rather than a list
PostgreSQL (and MySQL under `ONLY_FULL_GROUP_BY`) refuses
(`group_by_predicts_nothing_beside_an_ungrouped_aggregated_read`). `aggregates_rows` knows an aggregate by its
shape before its name — `OVER`, `FILTER (…)`, `WITHIN GROUP (…)`, a `DISTINCT` argument or an
`ORDER BY` among the arguments, which only an aggregate or a window takes, so a user-defined
aggregate written any of those ways needs no list — and only then by `GROUP_AGGREGATES`, on the
bare upper-cased name, for the plain `sum(x)` with no shape to tell it by
(`group_by_knows_an_aggregate_by_its_shape_and_its_name`). A name the list misses is grouped
by: without `JSONB_OBJECT_AGG`, PostgreSQL 16's `JSON[B]_OBJECT_AGG_STRICT`/`_UNIQUE`/
`_UNIQUE_STRICT` and SQLite's `JSONB_GROUP_ARRAY`/`JSONB_GROUP_OBJECT`, a
`jsonb_object_agg(…)` landed in the grouping list, which PostgreSQL refuses
(`group_by_leaves_out_every_json_aggregate`). A star, a projection with no key
left or a prefix that does not parse (a half-written `FROM`) predicts nothing.
**A join is predicted only where the schema leaves one join to make.** After an empty `JOIN `
the ghost is a row of the kind the popup's table list leads with — `join_targets_for`, asked of
the query's own tables (`query_scope`, below) — and only when that list holds a single table which `fk_edge_count` (the keys between two tables, in
either direction) finds joined to the tables in scope by a single key: `FROM orders o JOIN `
gets `customers ON o.customer_id = customers.id`, and from the other side
`FROM customers c JOIN ` gets `orders ON orders.customer_id = c.id`. Two reachable tables, two
keys to one table or nothing left to reach make which join is meant a choice, and the table
list is the place to make it; a table's key to itself counts twice, since which side is the
parent is that same choice. `join_on_guess` holds a table already written — `t`, `q.t` or
`db.q.t`, read by `joined_table` — to the same bar, offering `fk_predicate`'s condition only
when exactly one other table in scope is joined to it, by one key (`sole_fk_partner`, which
`USING (` asks too). **Only with an alias** (`JOIN t c ` or `JOIN t AS c `): after a bare `JOIN t `
the next word may well be the alias, and an unquoted reserved word in its place
(`JOIN t WHERE `, by `is_reserved_word`) is not one
(`an_empty_join_predicts_the_one_table_a_foreign_key_reaches`,
`an_empty_join_predicts_nothing_when_the_table_is_not_certain`,
`a_joined_table_and_its_alias_predict_the_on_condition`,
`a_joined_table_predicts_no_condition_it_cannot_be_sure_of`). **An empty `ON` is held to the
same bar** by `on_condition_guess`: the joined table and `sole_fk_partner`'s one table. It used
to be `join_condition`, the popup's row, which takes the first key `fk_predicate` finds — so
the ghost offered `s.bill_to = a.id` with a `ship_to` key beside it, `m.manager_id = e.id` for
a self-join, where which side is the parent is exactly the choice, and a partner from outside
the query (`an_empty_on_predicts_only_the_condition_one_key_decides`). The popup's own rows,
`join_condition` and `join_targets`, still take the first key — the popup is a list to choose
from, where the ghost is one offer and is held to one key.
**A name two tables share pairs nothing.** The foreign keys are held by bare name (`fks`), as
`pks` is, so beside `sales.orders` and its key to `customers`, `FROM dbo.orders o JOIN ` was
offered `customers ON o.customer_id = customers.id` — through a column `dbo.orders` does not
have. `fk_edge_count` answers 2, the count that makes a join a choice, when either name
`is_union` (under the synonym paragraph above), so every producer that pairs tables by a key
refuses: `on_condition_guess`, `join_target_guess`, `join_on_guess`, and `USING (` through
`sole_fk_partner` (`a_name_two_tables_share_predicts_nothing_from_their_union`). It is asked of
the bare name, so a namespace does not lift it as it does for an `INSERT`'s column list —
`FROM sales.orders o JOIN ` predicts nothing too, since bare-keyed edges cannot say which
`orders` declared them. And two same-shaped tables are not a union, so their keys still pool
under the one name: the condition then reads a column the table has, though not necessarily
a key it declared. `Catalog::pk_shared`, below, is the same refusal for a primary key.
**The tables these producers pair are the caret's own query's** — `on_condition_guess`,
`join_target_guess`, `join_on_guess` and the `USING (` arm all read `query_scope`, as do the
popup's `join_condition` and `join_targets`, where
`statement_scope` reads every query around the caret and the statement's own target with
them: from `local_scope_start`, at the last `SELECT` at
that depth (a set operation's last arm, the query after an `INSERT … `). The whole statement
held an `INSERT`'s target beside the `SELECT` feeding it, and an `UPDATE`'s beside a subquery
in its `SET`, so a key from the joined table to that target produced `invoices.customer_id =
c.id` in a query that cannot see `invoices`. It also drops a bare name a CTE stands in for,
whose columns and keys are not the catalogue table's; the names come from `cte_names`, a token
scan of the `WITH` list, because an empty `ON` never parses and `statement_scope`'s lexer
fallback reports no CTEs (`a_join_is_predicted_from_its_own_querys_tables`). The popup rows
kept the whole statement after the ghost had moved off it, and paired the `INSERT`'s target as if the query
could see it: `INSERT INTO line_items (order_id) SELECT o.id FROM customers c JOIN orders o ON `
was offered `line_items.order_id = o.id`, and `… FROM customers c JOIN ` was offered
`orders ON line_items.order_id = orders.id` (`the_popups_join_rows_read_the_joins_own_query`,
red before). `empty_on_join` still asks `statement_scope` whether the joined table is in scope
at all — the check that keeps a table inside a joined subquery from being taken for the join's
own — but returns only that table; which tables it is paired with is the caller's `query_scope`.
**And only the tables written before the caret** (`pairing_scope`, `query_scope` cut at the
caret): a later `JOIN` is not yet joined where this one's `ON` is evaluated, and a later
`UNION` arm is another query at the same depth, yet the lexer fallback read every `FROM`/`JOIN`
list up to the statement's end, so inserting a join above an existing one, or editing a
`UNION`'s first arm, was offered `i.customer_id = c.id` for an `invoices i` written below it —
*Unknown column … in 'on clause'* on every engine
(`a_join_pairs_with_no_table_written_after_it`, and the popup's half in
`the_popups_join_rows_read_the_joins_own_query`, both red before). What an alias must clear
is still the whole query's: `join_targets`' aliased rows and `table_alias_at` read `taken_names`
off `query_scope`, after the caret too, since a later `JOIN customers c` already holds `c`;
`table_alias_at` adds `statement_scope`'s tables, the enclosing queries' among them, since a
correlated subquery sees the outer aliases and `FROM orders o` inside `… FROM offices o WHERE
EXISTS (` shadowed the outer `o` (`a_subquerys_table_alias_clears_the_enclosing_querys`, red
before).
**Only a join that takes a condition is given one.** `empty_on_join` — split out of
`join_condition` and shared by both surfaces, so the popup row refuses too — answers only the
first `ON` after its `JOIN`, a second being MySQL's `ON DUPLICATE KEY` or PostgreSQL's
`ON CONFLICT ON`, and only where `join_takes_a_condition` says the join takes one: never after
`NATURAL`, and after `CROSS` only where `cross_join_takes_on` says the engine accepts the `ON`
— MySQL reads `CROSS JOIN t ON …` as an inner join and SQLite's join operator takes a
constraint whatever it is; PostgreSQL and SQL Server refuse it — asked as a capability through
an exhaustive match, as `has_apply_joins` is. `join_targets` asks `join_takes_a_condition` too,
of the `JOIN` its table slot follows: every row it offers writes `table ON …`, and after
`NATURAL JOIN ` it offered `customers ON …`, which every engine refuses. The producers that
would *add* an `ON` —
`join_target_guess`, `join_on_guess` and `USING (` — refuse after either word on every engine
(`cross_or_natural`): a `CROSS JOIN` is written without a condition and a `NATURAL` one writes
its own, so the ghost would add what the user's choice of join left out. The popup's table
rows are held only to what the engine accepts, so after `CROSS JOIN ` on MySQL or SQLite they
still offer an `ON` where the ghost offers none
(`a_cross_or_natural_join_predicts_no_condition`; `a_join_modifier_is_not_the_left_table_alias`
holds both popup surfaces to `join_takes_a_condition`, the table rows red before — on MySQL,
PostgreSQL and SQLite; its loop leaves SQL Server out, though it shares PostgreSQL's arm).
**Completion writes a table's alias only where one cannot be wrong.** `table_alias` makes it
from the name alone: the initials of the last `.` part's words — split at `_`, at anything
else `is_word_byte` does not take, and at a lower→upper camel boundary, so `order_items` and
`OrderItems` are both `oi`, and a non-ASCII letter continues its word (`straße_orders` is `so`,
not `seo`; `a_non_ascii_letter_does_not_start_a_word_of_the_alias`, red before) — lower-cased, leading digits dropped (`2fa_codes` is `c`), and numbered (`o2`, `on2`)
where that is taken, in any case, or could not stand bare after a table on some server of the
dialect: `reserved_anywhere`, `is_implicit_alias`, and T-SQL's `GO`, which no list reserves.
It is `None` where the alias would equal the name or there is no ASCII initial to make one
from, so what it returns never needs quoting. `table_alias_at` puts `alias_slot` in front of it
and reads what is taken from `taken_names` — every in-scope table's alias and name, less the
half-typed word, which `query_scope` reads in `FROM o|` as a table `o` and would push `orders`
to `o2`. The slot is narrow on purpose. The last clause keyword is `FROM` or `JOIN`, and the
statement opens with `SELECT`, `WITH`, `INSERT`, `REPLACE`, `UPDATE`, `CREATE`, `EXPLAIN` or
`(` — which leaves out `DELETE FROM`, where some engines take an alias and others refuse it,
and the `FROM` of `SHOW … FROM` and a cursor's `FETCH … FROM`, which name nothing to alias.
The head is the *statement's*, so a subquery inside a `DELETE` is given none either. A `FROM`
is refused too when the word before it — back over MySQL's `LOW_PRIORITY`/`QUICK`/`IGNORE` and
T-SQL's `TOP (n)` — is `DELETE`: the head cannot see a `DELETE` after a `WITH` or an `EXPLAIN`
or inside a routine or trigger body, and accepting a table there wrote `DELETE FROM orders o`,
which SQLite and SQL Server refuse (`no_table_alias_after_a_delete_from_anywhere_in_the_statement`,
red before). The word
is its item's first — only `name .` qualifiers between it and the keyword or the list's last
comma — so a table accepted into the alias slot of `FROM orders o|` gets no second alias and
`FROM (|` none. And nothing but `,`, `)`, `;` or the line's end follows it: whatever else does
may already be the alias, or the clause it would have to come before. `join_targets` asks the
same slot and fills `JoinTarget::aliased` — the alias, and the predicate rebuilt through it
(`customers c ON o.customer_id = c.id`), a second string because the alias hides the table's
own name from it — which `rank.rs`'s FK row inserts in place of the plain form. The ghost's
`join_target_guess` passes `join_targets_for` no taken names and stays unaliased
(`a_table_alias_is_written_at_a_from_or_join_slot`, `no_table_alias_outside_a_query_table_slot`,
`a_join_target_carries_its_aliased_form`).
**`alias_occurrences` is *Rename alias*'s walk**: the table alias under the caret — its
definition or any qualifier naming it — and every other place it is written in the statement
there, sorted. It reads the AST rather than the text because binding is the whole question: a
qualifier names the alias of the *innermost* query that defines one by that name, so a
subquery's own `o` shadows the outer one inside it while a correlated `o.id` reaches out — a
distinction a search for `o.` cannot make. The `alias_refs` walk takes every `Query` span as a
scope, with the statement's own range outermost (an `UPDATE`'s alias belongs to no query). A
definition is a `TableFactor::Table` or `Derived` alias, scoped by the innermost query containing
its *ident*, so a derived table's alias belongs to the query it is joined into, not to its own
body. A use is the qualifier of a `CompoundIdentifier`, an `o.*` (no expression, so read off the
select list in `pre_visit_query`, and off a `RETURNING` in `pre_visit_statement`), a function's
`count(o.*)` argument, a lock clause's `FOR UPDATE OF o` (outside sqlparser's `Query` span, so the
query's scope is widened to cover it), an `UPDATE`'s `SET o.x` target, and a one-part write
target — MySQL's and T-SQL's `DELETE o FROM orders o`, T-SQL's `UPDATE o SET … FROM orders o`.
Missed, each left one `o` behind and a statement the server refuses
(`an_alias_is_found_in_every_place_it_is_written`, S8.2-L1-07, red before). **A bare
identifier naming the alias refuses the rename** (`None`): on PostgreSQL it is the whole row
(`row_to_json(o)`), elsewhere a column that may share the name, and the walk cannot tell which.
A quoted name's range is the name inside its quotes, which stay. It is `None` where the text at the
caret does not parse to exactly one statement, or the caret is on no alias — a qualifier that is
a table's own name or a CTE's binds to no definition, and a select-list `AS` is not one, so none
of those is offered here — a CTE's name is *Rename CTE*'s, below, and an output column's alias
`output_alias`'s, after it
(`an_alias_is_found_with_every_reference_to_it`,
`an_alias_is_bound_by_its_own_query`, `an_alias_is_found_in_a_star_an_update_and_quotes`,
`no_alias_where_the_caret_is_not_on_one`). **`alias_rename_conflicts` is the capture check**
the same walk answers for a proposed name (R3-L5-03): `true` where the definition's own query
already exposes it (an alias, or a table written without one), where a query between a renamed
reference and the definition defines it — the correlated `o.id` renamed to the inner `i` binds
to the inner row, and a `DELETE … WHERE EXISTS` removes every row or none — or where a reference
inside the definition's query to an outer one by that name would find the renamed alias first
(`renaming_an_alias_to_a_name_that_would_rebind_is_a_conflict`, red before).
**`alias_rename` is *Rename alias*'s decision, and the capture check is its gate, not a warning
beside it.** The entry used to make `alias_occurrences` one multi-caret selection and let the
typing do the rename — and a name typed a keystroke at a time is never *finished* anywhere a check
could stand, so nothing asked the question above and the capture shipped. Now the editor asks for
the name in a dialog (`alias_rename.rs`, in `schemaic-ui`) and hands it here. The name is trimmed;
the answer is `AliasRenameRefusal::Empty` where nothing is left, `Conflict` wherever
`alias_rename_conflicts` says anything but `Some(false)`, and `NoAlias` where the walk finds no
alias — whose `message` is the sentence the dialog shows. Two more variants, `Changed` and
`ReadOnly`, are the *editor's* to return, never this function's (the context-menu passage under
`editor_pane.rs` has why); they live on the same enum so the dialog has one refusal to show.
Otherwise it is a `text_ops::MultiEdit`
replacing each occurrence's **whole** spelling, quotes included, with
`export::ident_if_needed(name, dialect)` — `Ord` on PostgreSQL is written `"Ord"`, and a quoted
`"o"` renamed to `x` loses its quotes — and leaving one caret at the end of the occurrence the
caret was on. That is why `alias_refs` keeps two views of one walk: `spans` is the raw ranges,
quotes and all, which the rename writes over, while `alias_occurrences` and `alias_name_at` (the
alias as its definition writes it, quotes off — what the dialog opens with) take them through
`unquoted` (`an_alias_rename_writes_every_occurrence`,
`an_alias_rename_refuses_a_capturing_or_empty_name`, `an_alias_name_is_its_definition_as_written`).
`alias_occurrences` has no production caller since; its tests go on pinning the walk.
**`definition_at` is Ctrl+click's jump**, and another reader of that walk: where the statement
defines the name at the caret, quotes off. It asks `alias_refs::definition` first — the range
`alias_name_at` reads — so a derived table's alias is found as a table's is, and a subquery's
shadowing `o` is the one a click inside it reaches. Where that walk finds no alias, a CTE is
looked up **by scope** where the statement parses — `alias_refs::cte_definition`, which is
*Rename CTE*'s `Ctes::bound` (below) at a one-part table reference under the caret, and
`cte_spans`' answer at a qualifier: the innermost query holding it that declares
the name, where the CTE is visible. **A bare word that is no table reference is never the CTE** —
a select-list `name` sharing a CTE's name is a column, the ambiguity *Rename CTE* refuses; the
jump once bound any word by name and scope and led there
(`a_bare_column_named_like_a_cte_does_not_jump_to_it`). So a CTE a subquery declares is
found, and shadows an outer one of the same name inside it while the outer one still answers
outside (`a_cte_declared_inside_a_subquery_is_found_by_scope`; only the leading `WITH` was read
before, so a nested one led nowhere and a shadowing one led to the outer). Where the statement
does not parse it falls back to the leading `WITH`'s names through `cte_name_tokens`, the token
walk `cte_names` already is rather than a second scanner. An unquoted name matches
ASCII-case-insensitively and a quoted one exactly on both paths, and a word after a `.` is never
the CTE — `x.n` is a column of `x`, whatever the `WITH` calls `n`. It is `None` on the
definition itself, so a click there falls through and still opens what the name names, and on
anything the statement does not define (`a_reference_leads_to_where_the_statement_defines_it`,
`no_definition_where_the_caret_is_not_on_a_reference`).
**`cte_rename` is *Rename CTE*'s decision**, shaped as `alias_rename`'s and sharing its refusals
and its edit — `rename_edit`, factored out of `alias_rename`, writes the one spelling over every
range and leaves the one caret — with `cte_name_at` as the menu's offer predicate and the
dialog's starting name, quotes off. Both read `alias_refs::cte_spans`, over a visitor of its
own, `Ctes`: each `WITH`'s `CteDecl`s — the name as written and whether it is quoted, its range, its body, its index, and
a scope that is the declaring query's span widened to hold the `WITH` — and every
`TableFactor::Table` reference, with whether its name is one part, since `s.c` is never the CTE
`c`. `Ctes::bound` binds a reference to the innermost declaring query that holds it where the
CTE is visible — inside a sibling's body only when declared before that sibling, in its own body
only when the `WITH` is recursive — and `None` is a real table, so the outer `c` a subquery's
`WITH c` does not reach is left alone, and a subquery's shadowing `c` is renamed without the
outer one (`a_cte_rename_follows_scope`). A qualifier `c.n` comes off the alias walk's `uses`,
and is the CTE's only where no alias of that name is in reach (`Walk::binding`) and the nearest
query exposing an unaliased table of that name exposes a one-part reference bound to it. It is
`None` — not offered — on anything not bound to a CTE, where the CTE's own `WITH` declares the
name twice, and where a bare identifier of its name stands in its scope: a column or
PostgreSQL's whole row, which, as with an alias, no walk tells apart. **So is a name the walk
cannot bind** (`Ctes::blind`): a one-part `INSERT` target — T-SQL inserts through a CTE,
PostgreSQL's target is a real table — and a `TABLE c`, neither a table factor. Missed before and
after the rename alike, the re-binding below could not notice them, and `INSERT INTO d` was left
under the old name (`a_cte_rename_refuses_a_reference_it_cannot_see`). **The capture check is a
re-binding, not a list of cases**: `cte_rename` writes the renamed statement, parses it, runs
`cte_spans` again at the moved declaration, and stands only if exactly the moved ranges come
back. A name that is a table the statement reads, a sibling CTE, an alias a qualifier would bind
to instead, or a duplicate in the one `WITH` changes that answer, and each is `Conflict` with no
rule of its own (`a_cte_rename_writes_its_declaration_and_every_reference`,
`a_cte_rename_refuses_a_capturing_name` — neither could fail first, the functions not existing).
The refusals' `message`s are worded for either rename: `NoAlias` says *There is nothing here to
rename any more.*, `Empty` *Give it a name.*; `Unsure` is the output-alias rename's alone (below),
and says a reference may read another column of the name, or read this one where the rename
cannot follow, and the loaded schema cannot tell. **Ctrl+click binds through the same `Ctes`**:
`cte_definition` is `Ctes::bound` at a reference and `cte_spans` at a qualifier, so the jump and the rename cannot disagree
about what a name names. Names compare by `same_name` — a quoted one on either side exactly, a
bare pair ASCII-case-insensitively — so on PostgreSQL, where `WITH "C"` and `FROM c` name two
things, the rename takes only the first; it once lowercased every name in a second visitor,
found while documenting it (`a_quoted_cte_name_binds_only_its_own_spelling`, red until the
two were one). Where the statement parses, the tree's answer is final — `definition_at` no
longer falls through to the leading-`WITH` token walk, which had reached a later sibling from an
earlier one's body.
**`output_alias` is *Rename alias*'s walk for an output column's alias** — `SELECT total * 2 AS
doubled … ORDER BY doubled` — which the table-alias walk above does not count as an alias at
all. Where such a name is visible, and what beats it there, are each engine's, and both decide
what a rename may touch. `ORDER BY` sees it on every engine **ahead of** a same-named column: as
a whole item everywhere, inside an expression (`ORDER BY t + 1`) on MySQL and SQLite alone
(`order_by_expression_sees_output_aliases`, the predicate completion already asks, so the two
cannot drift) — PostgreSQL 16 and SQL Server 2022 refuse that statement
and MariaDB takes it, checked live through `schemaic query`
(`an_alias_inside_an_order_by_expression_is_mysql_and_sqlites`). `GROUP BY`, `HAVING`, `WHERE`
and a join's `ON` see it only where `group_by_sees_output_aliases`, `select_alias_in_having`,
`select_alias_in_where` and `select_alias_in_join_on` say — SQL Server refuses a `GROUP BY` of an
alias, checked the same way, and `ON` sees one on SQLite alone, measured on 3.53.2 in memory:
`SELECT a.id*2 AS k FROM a JOIN b ON b.id = k` binds the alias, and `ON (SELECT k) > 0` does
too, where MySQL, PostgreSQL and SQL Server read only input columns there
(`a_join_on_reads_the_alias_on_sqlite_alone`) — and there **a column wins**: on PostgreSQL and
MariaDB, `GROUP BY name` beside a column `name` groups by the column, and SQLite's `SELECT a.x AS
id FROM a JOIN b ON b.y = id` answers *ambiguous column name* rather than reading the alias —
the columns are tried first, as in its `WHERE`. So a name in one of those clauses is the alias only where the sources
prove no column has it — a table's columns through `colres::table_columns`, `None` where the
catalogue cannot enumerate them, and a derived table's or a CTE's through the walk's own
outputs. Where they cannot, the reference is *unsure* and `output_alias_rename` refuses with
`AliasRenameRefusal::Unsure` rather than guess, while an `ORDER BY` reference, which the alias
wins, needs no catalogue at all (`an_unknowable_source_refuses_a_group_by_reference`,
`a_column_of_the_same_name_keeps_group_by_and_loses_order_by`). The bare names are collected by
sqlparser's own visitor at their `SELECT`'s own level rather than by a match over the expression
kinds — a kind the match left out would be a reference the rename leaves behind, to bind to a
same-named column. **A subquery's names are its own only where the engine keeps them so**
(`subquery_sees_output_aliases`: PostgreSQL and SQL Server). On MySQL and SQLite a subquery in
one of the alias's clauses sees it, behind every column on the way out — checked live on
MariaDB 10.11, MySQL 8.4 and SQLite 3.50, where `ORDER BY (SELECT t)` beside a column `t` reads
the column. `Tree::nested` takes each block inside the alias's through opaque subqueries alone
(a derived table's or a CTE's body on the way sees no enclosing alias — `nested_spot`), places
the subquery in the alias block's clauses by span (`Block::spots`, whose `Spot::On` covers the
block's own join conditions, a nested join's included — `join_ons`), and asks that clause's own
predicate, the select list's being `select_list_subquery_sees_earlier_aliases` (MySQL, and only
for an alias written before the subquery — after it is error 1247). A bare name there that
`resolve_bare` finds no source for is the alias; one a source inside or the alias's own `FROM`
has is the column; anything else — an unknowable source, an outer query's column, a clause the
walk does not place, a same-named alias on the way in — is `Unsure`
(`a_subquerys_names_are_left_alone`,
`an_alias_read_inside_a_subquery_is_renamed_where_the_engine_binds_it`). A join's `ON` was
once such a clause, so a subquery there refused the rename as unsure even on MySQL, which never
reads the alias there; placed, it is renamed on SQLite and left alone on MySQL. Each subquery
renamed adds a `Reach` whose rivals are its own sources' columns. **A window's `ORDER BY` and
`PARTITION BY`** (`Block::windows`, from `window_idents`) read the alias ahead of a column on
MariaDB alone — MySQL 8.4 answers *Unknown column 't' in 'window order by'* — so
`window_sees_output_aliases` asks `Catalog::flavour`, and a MySQL-family server it cannot name
leaves the rename `Unsure` (`a_windows_order_by_reads_the_alias_on_mariadb_alone`).
**`output_alias::find` builds a `Tree` of every `SELECT` in the statement** — a sqlparser
`Visitor` over `pre_visit_query`, `pre_visit_select` and `pre_visit_table_factor`, keyed by node
address, since the borrow each callback is handed cannot be stored — holding each block's
sources (a table with its catalogue columns, a derived table, a CTE — bound through
`alias_refs::cte_bindings`, which is `Ctes::bound`'s answer, so this walk, the jump and *Rename
CTE* cannot disagree about which CTE a name reads — an opaque factor), its outputs, its
own-level column references with the
clause each stands in, and its query's `Role`: the top level, an opaque subquery read by no
name, a set-operation branch, a derived table's body with its `AS d(x)` list if it has one, a
CTE's body. Any `SELECT` but a branch offers its aliases — a subquery's, a derived table's, a
CTE body's. A branch's does not, since the first branch's names are the union's and the walk
would find half of them (`a_union_branch_or_a_bare_column_offers_nothing`); nor does a name two
outputs share, which one a reference means being the engine's business. **An alias a derived
table or a CTE defines is a column its readers read, and `Tree::reach` follows it there**: a
qualified `d.total`, bound by `resolve_qual` at the nearest scope outwards, and a bare `total`
where `resolve_bare` proves the reader's source is the only one with it — one source having it
beside one whose columns are unknown is that one, since were the unknown one to have it too the
server would refuse the name as ambiguous
(`a_derived_tables_alias_is_renamed_where_the_outer_query_reads_it`,
`a_bare_outer_reference_is_renamed_when_only_the_derived_table_has_it`,
`a_ctes_alias_is_renamed_where_the_ctes_readers_read_it`). A correlated subquery is a reader too
(`correlated_sibling_and_shadowed_readers_bind_by_scope`). A reader's `ORDER BY` reads that
reader's own alias of the name ahead of the column, so such a reference is left alone. The walk
goes on outwards where a reader passes the column on under the same name — `SELECT d.total`,
`*`, `d.*` — and is itself a derived table or a CTE (`a_column_passed_on_is_followed_outwards`),
and stops at a reader that names the column itself — its own alias, `AS d(x)`, a CTE's column
list — and at the top level or an opaque subquery
(`a_column_list_or_an_outer_alias_stops_the_rename`). A caret on any reference it reaches finds
the alias (`a_rename_starts_from_the_outer_reference`), and `Found::maybe` holds the unsettled
ones' spans, so a caret on one of those finds it too and is refused rather than offered nothing.
**Where a reader cannot be seen whole the rename is `Unsure`, never partial**: a derived table
no walked `SELECT` reads (a write's `FROM`, a `MERGE … USING`), a CTE whose name also stands
where no `FROM` reads it (`Tree::blind` — `TABLE c`, an `INSERT` target, an `UPDATE`'s tables),
a `USING` or `NATURAL` join in the reader, a bare name with an unknown source on the way out,
and a union that passes the column on and is itself read — a top-level unordered one is fine,
its names being the result's (`a_reader_the_walk_cannot_settle_refuses`,
`a_union_reading_the_column_is_renamed_only_at_the_top`). `output_alias::conflicts` is the
capture check: `Conflict` for a name another output has — an alias, or a column written
unaliased — or one written bare in `ORDER BY` that would now bind to the renamed alias; where
some reference sits behind columns, a column of the new name is a `Conflict` too, since it would
win there, and a catalogue that cannot say is `Unsure`
(`a_name_that_would_capture_is_a_conflict`). Each reader adds its own, one `Reach` apiece in
`Context::outer`: a name written bare there, another of its outputs, and — where a bare
reference there is renamed — another source's column, an unknowable one being `Unsure`
(`a_name_the_outer_query_already_uses_is_a_conflict`). A respelling skips the bare names and
the rival columns only where `names_match` says the new name is the old one — every bare name
there already reads it; the readers' outputs are still checked. It used to skip them on any
change of case alone, which on PostgreSQL let a quoted `"Total"` become `total` beside a bare
`total` outside already bound to another column — ambiguous after the rename. Each
`Reach::bare` name keeps whether it was quoted, so the alias's own quoted reader is no rival
to its own respelling (`a_case_only_rename_collides_where_quoting_keeps_case`). **Names compare as the engine compares column names**
(`intel::same_name`, which `output_alias::names_match` — asked by `Name::same`, the
qualifiers' `own_source` and `conflicts` — and the grid's sort tie-breaker share): where
`SqlDialect::folds_unquoted_names` (PostgreSQL) a bare side folds to lower case and the two then
compare exactly, and elsewhere quoting changes nothing — MySQL's aliases and SQL Server's (by
its collation) match whatever the case of **every** letter
(`SqlDialect::names_fold_unicode_case`, so `été` is `Été`, checked live on MySQL 8.4, MariaDB
11 and SQL Server 2022), SQLite's whatever the case of an ASCII one. Byte-exact
wherever one side was quoted, the rename left `ORDER BY dbl` behind `` AS `Dbl` ``, which
MariaDB then refused (1054), and missed a MySQL capture by `` `X` ``
(`a_quoted_side_matches_as_the_engine_compares_names`). The edit is `rename_edit`'s, quoting through
`ident_if_needed` as the other two renames do (`a_quoted_alias_is_renamed_where_it_is_written_quoted`);
the dialog's starting name is `output_alias_name_at`'s, and the catalogue is the caller's — the
editor builds it from the loaded schema when asked (under `editor_pane.rs`).
**`usages_at` is *Find usages*' answer, and it is the three walks above asked in turn** — the
table alias (`alias_refs::spans`), then the CTE (`alias_refs::cte_spans`), then the output
alias — so what it lists is exactly what the matching rename would write: the name as its
definition writes it, quotes off, and every whole spelling, in order
(`usages_are_found_for_a_table_alias_a_cte_and_an_output_alias`) — an output alias's readers
outside its own query included (`usages_follow_a_derived_tables_alias_outwards`). It keeps their refusals and
adds one: an output alias with an unsure reference answers `None` here, where *Rename alias*
still opens and says `Unsure`, because **a list that would miss a usage is not offered**.
`usage_lines` turns the spans into rows — each usage's line, 1-based, and that line's text
trimmed and cut at 160 characters (`a_usage_is_listed_by_its_line`;
`find_usages_lists_the_definition_and_its_references` pins an output alias's list).
**A `(` is filled from the schema only where the schema names one answer.** `paren_contents`,
asked right after `insert_columns`, answers three openers through the same `column_list`.
`REFERENCES t (` gets `t`'s primary key from `Catalog::key_columns_of` — its columns in the
table's case but in the key's **declared** order, which `Catalog::build` reads off the primary
index and falls back to the `primary_key` flags in column order only where there is none. A
foreign key's referenced columns follow the key's order — MySQL wants them in the referenced
index's — so `PRIMARY KEY (b, a)` must give `b, a`, and table order gave `a, b`
(`references_names_a_composite_key_in_its_declared_order`). The key must also be as wide as the
foreign key it completes — `referencing_width` counts the names in a `FOREIGN KEY (a, b)`
written straight before the `REFERENCES`, else 1, a column's own — since a one-column
`a int REFERENCES lines (` was offered `order_id, line_no)`, a count the server refuses
(`references_names_a_key_only_as_wide_as_the_foreign_key`). It predicts nothing when no key is
declared; when the reference names another loaded database's table, whose key `Catalog::pks`
(active-database scoped) does not hold; or when two tables in scope share the name with
different keys — `sales.orders` and `dbo.orders`, no key counting as a different one. `pks` is
keyed by the bare name and keeps whichever was indexed last, so the reference to the other one
got that one's key; `Catalog::pk_shared` records such names, comparing every in-scope table's
key (an empty one included), and `key_columns_of` refuses them
(`references_to_a_name_two_tables_share_predicts_nothing`). **So does the diagnostics pass's
`only_full_group_by` check** (`group_by_check`), whose functional-dependency exemption — the
grouping set holds the table's whole primary key — read that key through `pk_of`, keyed the
same way: beside `ops.employees` (key `dept_id`), `SELECT dept_id, name FROM hr.employees GROUP
BY id`, grouped by its own key, was flagged twice against the other table's. The check is
conservative by design, skipping what it cannot judge, and which key is this source's is exactly
that, so a shared name skips the source outright — the cost is a true warning missed on such a
table (`diag_only_full_group_by_skips_a_name_two_tables_share`). `INSERT … ON CONFLICT (` gets the
target's key through the same `key_columns_of` (`conflict_key`, the target read by the same
`insert_target` as the column list), but only when the `INSERT` writes every key
column — all of them in its column list, or no list at all, which writes every column. One
that leaves the key to the server conflicts on some other unique column, which the catalogue
does not record, so naming the key there would be a guess. `JOIN t [alias] USING (` reads the
joined table through `joined_table` and `sole_fk_partner` as `join_on_guess` does — an alias
is optional, since `USING` already stands where one would — and gets the columns of that one
key (`Catalog::fk_edge_between`) only when they are named alike on both sides, which is what
`USING` requires: `orders.customer_id → customers.id` predicts nothing
(`references_predicts_the_referenced_tables_key`,
`on_conflict_predicts_the_key_the_insert_writes`,
`using_predicts_a_key_named_alike_on_both_sides`).
**An upsert's update list is every column the `INSERT` writes, set from the row it tried to
insert, less what identifies the row.** `upsert_assignments`, asked right after
`paren_contents`, answers only with the update word last — MySQL/MariaDB's
`… ON DUPLICATE KEY UPDATE ` (the word `keyword_partner` offers after a hand-typed
`ON DUPLICATE KEY`) and PostgreSQL/SQLite's `… ON CONFLICT (k) DO UPDATE SET ` — and reads the
columns off the `INSERT`'s column list — read past a PostgreSQL/SQLite target alias
(`INSERT INTO t AS c (…)`, `past_insert_alias`) — else the table's
`Catalog::insert_columns_of`, each quoted through `export::ident_if_needed`. **The table's
columns stand in only for a `VALUES` row as wide as the table** (`values_row_width`): an
`INSERT … SELECT`, `DEFAULT VALUES` or a short row leaves the columns it does not reach to their
defaults, and an update naming them sets the stored row's values to those — the predicted
`email = EXCLUDED.email` wiped a PostgreSQL 16 row's `email` this way — so with no list and any
other source nothing is predicted. MySQL's arm writes `a = VALUES(a), b = VALUES(b)`, or
`a = new.a` where MySQL 8's row alias (`VALUES (…) AS new ON DUPLICATE KEY UPDATE`) is written —
found by `upsert_row_alias`, the scope builder's own reading (the `AS` after the `VALUES` rows,
at the statement's depth), since an `INSERT … SELECT … FROM src AS s` puts the source table's
alias in the same place and `s.a` is that table's column, not the row inserted.
`VALUES()` is the default because it is MariaDB's only spelling and MySQL 8 still accepts it,
though deprecated; the alias is MySQL 8's alone. The conflict arm writes `a = EXCLUDED.a`. Both
write `VALUES`/`EXCLUDED` in the case `UPDATE`/`SET` was typed (`in_typed_case`). What
identifies the row is the conflict target's own list where there is one, else — MySQL, or a
target named `ON CONSTRAINT` — the primary key through `Catalog::key_columns_of`. **That key is
subtracted only where known**: with none declared, or a name `pk_shared` refuses, nothing is,
so a key in the column list is assigned like any other column. Nor does MySQL's arm leave out
a unique key other than the primary one, which also fires `ON DUPLICATE KEY` — the catalogue
does not record those (`conflict_key`'s limit, above). It predicts nothing when nothing is left
to set (an `INSERT` of the key alone); when the update word is not last — an assignment
already begun, a `DO NOTHING`; or when `insert_columns_of` cannot name the table's columns —
a table it does not know, or a name two tables share with different columns (`is_union`) —
**column list or no**: such a table's key could not be told apart, and an `INSERT INTO
nowhere (id, a)` got `id = VALUES(id)` with the rest until the catalogue was asked on that path
too. The shapes are
matched on the text, not the dialect (`an_upsert_predicts_its_assignments`, red before;
`an_upsert_predicts_no_assignment_it_cannot_list`, the refusals).
**An operand's partner waits for the operand to be finished.** `operand_partner` answers where
what has just been written is a construct's operand and the next word is fixed, each in the
case its keyword was typed: `BETWEEN x ` → `AND` (`between_and`, once `x` is finished at
`BETWEEN`'s own depth and no `AND`/`OR` follows it — and, in a window frame, a
`ROWS`/`RANGE`/`GROUPS BETWEEN`, only once the bound ends on `PRECEDING`, `FOLLOWING` or
`ROW`, since `ROWS BETWEEN UNBOUNDED `, `1 ` and `CURRENT ` are finished operands and were
each offered `AND`); `CAST(x ` — `TRY_CAST`, `SAFE_CAST` — →
`AS` when the innermost open paren is the call's, with one finished operand and no top-level
`AS` or comma, and `EXTRACT(field ` → `FROM` on exactly one word (`call_keyword`);
`SELECT * `, `SELECT DISTINCT|ALL * ` and `SELECT t.* ` → `FROM` (`star_from`) — a star takes
no alias, so nothing else can come, where `SELECT a ` may be followed by its alias and
predicts nothing; T-SQL's `TOP n`, `TOP (n)`, `PERCENT` and `WITH TIES` may stand between the
`SELECT` (or its `DISTINCT`) and the star, and a bare count is no token, so its digits are
skipped in the text — `SELECT TOP 10 `, the star not yet written, still predicts nothing;
a CTE's name, right after `WITH`, `WITH RECURSIVE` or a depth-0 comma → `AS`
(`cte_as`; not T-SQL's `WITH XMLNAMESPACES `, which declares namespaces rather than a CTE and
was offered `AS` — `with_xmlnamespaces_predicts_no_as`), until a depth-0
`SELECT`/`INSERT`/`UPDATE`/`DELETE`/`MERGE`/`VALUES` shows the
query proper has begun — `AS ` and not `AS (`, because the user's own `(` auto-pairs and an
inserted one would not; `DELETE FROM t ` (`t`, `q.t` or `db.q.t`, nothing more) → `WHERE`
(`delete_where`), offered as a safety as much as a prediction — after the statement's own
`DELETE`, its first word or the first at depth 0 behind a `WITH` list, and past T-SQL's
`TOP (n) [PERCENT]`, so `WITH x AS (…) DELETE FROM t ` and `DELETE TOP (10) FROM t ` get it
too; a `WHEN`'s condition → `THEN` (`when_then`), once it is a finished operand and no `THEN`
has followed the last `WHEN` — `CASE WHEN total > 1 `, a simple `CASE status WHEN 'open' `, a
`MERGE`'s `WHEN [NOT] MATCHED ` — but never in a `CREATE … TRIGGER`, whose `WHEN (…)` is
followed by the trigger's body rather than a `THEN`; that guard reads the statement's first
words, so a `CASE` inside a trigger's body gets no `THEN` either; a named index → `ON`
(`index_on`), after `CREATE [UNIQUE|CLUSTERED|NONCLUSTERED|FULLTEXT|SPATIAL] INDEX
[CONCURRENTLY] [IF NOT EXISTS] name `, the name read as `written_table` reads a table's and
none of `ON`, `USING`, `IF` or `CONCURRENTLY`, while an unnamed `CREATE INDEX ` predicts
nothing, since its name may still come (these two arms, and the `TOP`, `WITH … DELETE` and
routine-signature shapes, are `the_words_as_certain_as_the_others_are_predicted_too`); and
`GRANT`/`DENY … ON obj ` → `TO`, `REVOKE … ON obj ` → `FROM` (`grantee_word`), once the
text after `ON` ends in a name character, `*`, a closing quote or the `)` closing a routine's
signature (`GRANT EXECUTE ON FUNCTION f(int) `), with every paren it opened closed
(`depth_after`), not while only the object's kind is written (`PRIVILEGE_OBJECT_KINDS` —
`TABLE`, `SCHEMA`, `ALL`, `IN`, …) and not once `TO`/`FROM` is. The paren check is
load-bearing: the last character alone offered `TO` inside `FUNCTION f(int `, a signature
still being written (`the_new_answers_hold_their_guards`, red first).
*Finished* is `operand_finished`: something written, its parens closed (`depth_after`), and
the last thing neither an operator character nor a word that wants another operand
(`OPERAND_PENDING` — `AND`, `OR`, `NOT`, `IS`, `IN`, `LIKE`, `AS`, …), so `BETWEEN 1 + `,
`BETWEEN f(1 ` and `CAST(f(total ` predict nothing (`an_unfinished_operand_predicts_nothing`).
The list once knew too few infix words: `CAST(created AT TIME ZONE ` and MySQL's
`CAST(total DIV ` were offered `AS`, PostgreSQL's `BETWEEN SYMMETRIC ` `AND`. It now holds
`SYMMETRIC`/`ASYMMETRIC`, `AT`, `ZONE`, `DIV`, `MOD`, `XOR`, `REGEXP`, `RLIKE` and `INTERVAL`,
with `AT TIME` pending as a pair; and an `INTERVAL` followed by a number is pending too, since
MySQL's `BETWEEN now() - INTERVAL 1 ` still wants its unit. **A pending word counts only if
the text ends on it**, not merely the tokens: numbers and strings are not tokens, so
`BETWEEN SYMMETRIC 1` ends on `SYMMETRIC` among the tokens and on `1` in the text, and is
finished — the same reading that keeps `INTERVAL '1 day'` finished while `INTERVAL 1`, whose
text ends in a digit after that last token, is not (`between_waits_for_a_whole_bound`,
`an_operand_waits_for_its_infix_words_and_its_unit`). It still knew too few: `CASE WHEN a IS
DISTINCT ` ghosted `FROM `, and taking it ghosted `THEN ` straight after — `IS DISTINCT FROM
THEN` in two Tabs (S8.4-L1-03). `SIMILAR`, `GLOB`, `MATCH`, `OVERLAPS`, `SOUNDS` and `MEMBER`
joined the list; `DISTINCT FROM`, `SIMILAR TO` and `MEMBER OF` are pending as pairs, like `AT
TIME`, so a lone `FROM`, `TO` or `OF` (`EXTRACT(year FROM `) is not; and `@` and `#` joined the
operator characters for PostgreSQL's `@@`, `<@`, `@>` and `#` — not `?`, which ends a
placeholder as often as a `jsonb` operator. `scan_clauses` reads that `FROM` as the
operator's too (`is_distinct_from`, an `IS [NOT]` before the `DISTINCT`), or `ON a.x IS
DISTINCT FROM LEFT ` was a `FROM` clause and ghosted `JOIN`
(`an_operand_waits_after_distinct_from_and_the_other_infix_words`, red first;
`a_finished_operand_predicts_the_word_its_construct_takes`, `a_star_projection_predicts_from`,
`a_common_table_expressions_name_predicts_as`, `a_delete_target_predicts_where`,
`a_privilege_object_predicts_its_grantee_word`).
**A construct's own word is offered where the construct cannot stand without it.**
`clause_partner`, asked last, reads a closed call and a written count. A window-only
function's call (`WINDOW_ONLY` — `ROW_NUMBER`, `RANK`, `DENSE_RANK`, `PERCENT_RANK`,
`CUME_DIST`, `NTILE`, `LAG`, `LEAD`, `FIRST_VALUE`, `LAST_VALUE`, `NTH_VALUE`) → `OVER` —
`OVER ` and not `OVER (`, for the reason `cte_as` answers `AS `: the user's own `(` auto-pairs
and an inserted one would not. An ordered-set aggregate's → `WITHIN GROUP` where
`takes_within_group` says the engine has one: `PERCENTILE_CONT`/`PERCENTILE_DISC` on
PostgreSQL, SQL Server and the MySQL dialect (MariaDB's), and PostgreSQL's `mode()` besides;
never on SQLite, whose `percentile_cont(Y, P)` is an ordinary aggregate. A typed `WITHIN` gets
`GROUP` from `keyword_partner` — the word's only use — and `WITHIN GROUP (` gets `ORDER BY`
here. A table-level `FOREIGN KEY (a, b)` → `REFERENCES`. And a count in a query — `in_a_query`,
factored out of `groups_rows`, wanting a `SELECT` at depth 0 in the word's own query — gets the
`ROWS` its engine requires: T-SQL's `OFFSET n` → `ROWS` (`offset_takes_rows`, T-SQL alone —
PostgreSQL makes the word optional, and MySQL and SQLite write a bare `OFFSET n` after their
`LIMIT`), and `FETCH NEXT|FIRST n` → `fetch_rows_tail`'s answer: `ROWS ONLY` on T-SQL, its only
ending; `ROWS ` on PostgreSQL and the MySQL dialect (MariaDB's), where `ONLY` and `WITH TIES`
both follow and which one is the user's choice; nothing on SQLite, which has no `FETCH` clause.
A count is no token, so the text must end in a digit after the word — which is what keeps a
cursor's `FETCH NEXT FROM` out. `takes_within_group`, `offset_takes_rows` and
`fetch_rows_tail` are asked as capabilities through an exhaustive match, as `has_apply_joins`
and `has_full_joins` are, so a new engine has to answer them. A name a routine is being
declared under (`FUNCTION`, `PROCEDURE`, `AGGREGATE` or `ROUTINE` before it) is no call and gets
nothing, nor does a quoted name, **nor an object's name before its column list** — a word of
`NAMES_AN_OBJECT` (`INTO`, `INSERT`, `TABLE`, `EXISTS`, `VIEW`, `INDEX`, `ON`, `REFERENCES`,
`JOIN`, `TRIGGER`, `COPY`) in front of it, read past a qualifier: a CRM table called `lead`
ghosted `over` after `INSERT INTO lead (name, email) ` (S8.4-L1-01). `FROM` is not one of them,
since `IS DISTINCT FROM lag(x) OVER …` is an expression. PostgreSQL's `RANK`, `DENSE_RANK`,
`PERCENT_RANK` and `CUME_DIST` given an argument are hypothetical-set aggregates and get
`WITHIN GROUP` instead (`ranks_hypothetically`, a capability like the rest;
`a_table_named_like_a_window_function_takes_no_over`, red first). Each answer is in the case
its word was typed
(`a_construct_predicts_the_words_it_cannot_do_without`, red first on
`SELECT id, ROW_NUMBER() `; `the_constructs_words_are_offered_only_where_they_are_required`,
the guards — a function that is no window function, SQLite's two-argument `percentile_cont`,
an `OFFSET` that takes no `ROWS`, a cursor's `FETCH NEXT`, an unfinished call or key list —
which passed before, as guards do). Neither pins the declared-routine or quoted-name guard.
The UI asks `predict`
over the offer catalogue (under `completion.rs`) and only while the `editor_predictions`
setting is on — Settings → Editor → *Inline predictions*, on by default. **The gate is a
function of its own, `may_predict_at`**: every check above that reads only the caret's line and
statement — the rest of the line, the leading indent, the character before, the ended
statement, noncode — and no schema, so a caller can ask it before building the `Catalog`
`predict` takes, and `false` means `predict` is `None` whatever the catalogue. `predict` runs
the same gate (`predict_gate`), then its producers, then `finish_prediction`, which holds the
guess to what follows the caret — the next-word and `may_precede` checks, which need the guess
and so cannot be in the gate. Keep the split that way: a check moved from the gate into a
producer still predicts correctly but leaves `may_predict_at` passing where nothing could be
offered, and one moved the other way that needs the catalogue cannot be asked without it
(`the_gate_refuses_only_where_nothing_could_be_predicted`, which also asks that the gate pass
wherever `predict` answers). **The gate finds the caret's statement in one place,
`caret_statement`**, which `Recall::at` asks too: off a `sql::StatementIndex` of the text where
the caller passes one — `predict_indexed`, `may_predict_indexed`, `Recall::at_indexed`, the
forms the editor calls — and by the walk from the document's start (`sql::statement_range_ended`)
otherwise, or where the index's `text_len` is not the text's. That walk is a document's lex per
call, which the plain forms keep for tests and one-off callers;
`the_indexed_prediction_answers_what_the_walk_does` holds the two forms equal at every caret of
a corpus on every engine, with an index built whole and one carried across an edit.
`predict_tests` covers
each producer and gate, `an_auto_paired_closer_after_the_caret_still_predicts` the `)` one.
**`predict_from_history` is the ghost remembered rather than reasoned**, asked only when
`predict` has nothing, under the same *Inline predictions* switch and a second of its own,
*Predictions from past statements* (under `completion.rs`): the rest of the caret's line from
the first statement in `past` that begins with the statement typed so far, its leading
whitespace trimmed. `past` is any text worth remembering from, in the order the caller trusts
it — the UI gives it runs, then snippets, then open tabs. **It is two steps under one name**:
`Recall::at(sql, caret, dialect)` runs the caret's gate once, and `Recall::from(text)` asks it
of one text; `predict_from_history` is `at` then `find_map(from)`. The split is for a caller
whose texts are not at hand at once — each open tab's sits behind its own signal — so the gate
runs once a keystroke rather than once a tab. Its gate is stricter than `predict`'s.
At least two words must be typed — one word starts every run, so matching it is a guess, not a
memory; nothing may follow the caret in the statement but the `)`s bracket pairing made and a
`;`, not even the next clause on another line, since a statement being edited in the middle is
not the one run before; and the caret wants whitespace or `(` before it and code under it
(`caret_touches_noncode`, as above), and the statement not already ended behind it
(`sql::statement_range_ended`, which `predict` refuses on too). **Inside an auto-paired `(…)`** the remembered line must
end in as many `)` as follow the caret, and is offered without them: `IN (|)` over a past
`IN (1, 2)` offers `1, 2`, while one that runs on past the paren (`IN (1, 2) ORDER BY id`)
offers nothing, since all of it would land inside the `)`. That spot used to be refused outright
(`after == ")"`), though `predict` answers there. The match is
SQL's, not the bytes' (`strip_prefix_loose`): keywords and bare names in any case, and a
white-space run matching one of any width or kind — `select *⏎from orders ` continues a past
`SELECT * FROM   orders WHERE …` — but a string, quoted name or comment exactly as written,
since `'Open'` is not `'open'`; a run ending the typed text takes all of the stored one, so the
offer starts at a word (`recall_ignores_case_and_spacing_but_not_inside_quotes`; it was an
exact prefix until then). It reads text with `\r\n` made `\n`, and a stored run of several statements is cut by
`sql::statement_ranges` so that each is offered, each read from its first byte of code
(`skip_leading_noncode`): the range `statement_ranges` gives includes a comment heading the
statement, so a run saved as `-- weekly report⏎SELECT …` never matched at all. **The statement
typed is read the same way** (in `Recall::at`), and was not until a comment heading it —
`-- weekly⏎SELECT * FROM orders ` — turned recall off, the stored statement being read past its
comment while the typed one kept it; the comment's words no longer count toward the two the
gate wants either (`a_comment_heading_the_typed_statement_is_skipped`).
It answers one line, its trailing `;` dropped:
a statement written over several continues line by line as Enter starts each, so a ghost never
needs a newline — which it could not carry, since `inline_diff` draws none that holds a `\n`.
**That line ends at the first line end *in code*** (`first_code_line`, walking the text through
`skip_noncode`), and a run whose block comment or string crosses a line end before it offers
nothing. The line was once cut at the first `\n` wherever it sat, so a past
`SELECT * FROM orders /* multi⏎line */ WHERE id = 1` offered `/* multi`, and Tab left that
comment open over the rest of the buffer — a multi-line string likewise. **So does one the
text's end cuts off unclosed**: an open tab stops mid-statement, and a tab ending
`WHERE note = 'abc` ghosted that and left the `'` open (S8.4-L1-04). `skip_noncode` ends an
open span at the text's end as it ends a closed one there, so a span reaching the end is asked
again with `\nx` after it: an open one runs on to the new end
(`a_remembered_line_never_ends_inside_an_unclosed_span`, red first). A line comment ends
its own line, and is offered whole **only where nothing follows the caret on it**
(`Recall::followed`): where the statement's `;` or the `)`s bracket pairing made sit there, a
trailing `--` (or MySQL's `#`) is cut off (`before_line_comment`), since Tab would comment out
the `;` after the caret and merge the next statement into this one — and inside a paired
`(…)` the line could not end in its `)` at all. A line that is nothing but the comment then
offers nothing (`a_remembered_line_comment_never_swallows_what_follows_the_caret`).
**A statement `sql::carries_credential` flags is never offered, from any text** — asked in
`Recall::from` itself, not left to the source. `history::push` keeps such a statement out of
the history, but a tab or a snippet holds whatever was typed into it, so a tab holding
`CREATE USER bob IDENTIFIED BY 'S3cret'` ghosted the password after `CREATE USER bob `; the
statement beside it in the same text is still offered
(`a_statement_carrying_a_credential_is_never_remembered`).
**It runs on every keystroke, so what it reads is bounded.** Each space at a statement's end
used to re-lex every stored run of the connection — up to `history::MAX_PER_CONN`, then 50 (now
200), each stored whole — and a review measured 432 ms a keystroke, in a release build, after a
Run All of a mysqldump. Now a text longer than `RECALL_MAX_BYTES` (256 KiB) is skipped; one that does not
hold the typed text — searched as loosely as it is matched, by
`text_ops::contains_collapsed_ignore_ascii_case` over `Recall::needle`, the typed text with its
white space collapsed — is passed over; and only one that does is given its `\r\n` copy (where it holds a `\r`) and
lexed by `statement_ranges`. **The caret's own tab is not lexed again**: that search always
finds the typed text in it, so it was always lexed — and the pane's `sql::StatementIndex`
already holds its statements. `Recall::from_ranges(text, ranges)` reads ranges handed in
(`StatementIndex::statement_ranges`, `statement_ranges`' answer from the index's boundaries, which
`a_statement_index_answers_what_the_walks_do` holds equal), and `completion::recall` hands it the
own tab's (identified by its `query` signal); a text with `\r` still goes through `from`, its
offsets being the normalised text's (`recall_from_known_ranges_answers_as_a_lexed_read`). **The cap was 16 KiB until open tabs were read too**: a scratch
tab is routinely longer than that, and at that cap would not have been read at all. The
substring search is what keeps a keystroke cheap — most texts do not hold the typed line — and
the cap is left to keep a dump or seed script run whole from being lexed whenever one does. The
cap is on the whole text, not on any one statement in it: a short query run as part of a script
over 256 KiB is not remembered — that is the trade
(`history_prediction_tests::a_statement_run_before_predicts_the_rest_of_its_line` and
`the_newest_statement_that_fits_is_the_one_predicted`, red against a stub first;
`history_predicts_nothing_it_cannot_continue` the gate;
`a_remembered_line_never_ends_inside_a_comment_or_string`,
`a_run_that_opens_with_a_comment_is_still_remembered`,
`a_remembered_line_fills_an_auto_paired_paren` and
`a_run_too_long_to_scan_per_keystroke_is_not_remembered`, each red against the body before
them; `a_statement_deep_in_a_long_text_is_remembered`, red at the 16 KiB cap). Which texts it
is given, and in what order, is the UI's choice (under `completion.rs`).
