# `core/sqlfmt.rs`

`sqlfmt.rs` — `format_sql` (Ctrl+Alt+L pretty-printer): re-flows whitespace/indent/line-breaks
"block" style, **preserving keyword case** itself; built on `skip_noncode` so comments/strings/
backtick idents pass through untouched; indent follows editor tab-width/soft-tabs.
A line's indent is an `Ind { lv, col }` — `lv` indent units, then `col` spaces of alignment, two
parts because a unit may be a tab and an alignment never can be; the block layout moves only
`lv` (`ind + 1` adds a level, `up()` steps one out, never below the margin), and `col` is the
river's (`FormatStyle::river`, below). `Fmt::run` is a loop
over `Fmt::step(toks, k)`, which lays out one token — and any follower it takes with it, a
head's `BY`, a `BEGIN`'s `TRY` — and answers the next index, so a layout can be tried and taken
back; the per-token arms below are `step`'s.
**Keyword case is a setting now, and `format_sql` still does not know it** — the recasing is
`apply_keyword_case(sql, dialect, KeywordCase)`, applied over `format_sql`'s output by
`format_range` (and so by `format_statement`), the two functions a Format writes through.
`KeywordCase` is `Keep` (the default, labelled *As typed*: no token's text changes, the
formatter as it always was), `Upper` or `Lower`. **A keyword is deliberately narrow**: an
unquoted `Kind::Word` that every server of the dialect reserves (`intel::is_reserved_word_on`
with `ServerFlavour::Unknown`, so on a MySQL tab MariaDB's freeing of `rank` keeps it a name),
**or** one of this module's own layout keywords (`is_keyword` — clause heads, joins, `BY`, `AND`
…), which the formatter already reads as a keyword wherever it stands. The second half is there
because PostgreSQL reserves neither `BY` nor `INSERT`, and the reserved list alone wrote
`ORDER by`. It goes no wider than that on purpose: a general keyword list would recase names,
and an unquoted MySQL table called `status` is case-sensitive on a Linux server. For the same
reason a word beside a `.` is left alone — MySQL takes a reserved word as a qualified name
(`shop.order`). Strings, comments and quoted names are tokens of their own and never words
(`keyword_case_recases_keywords_and_nothing_else`, both halves). **"Wherever it stands" was
too wide for the second half**: several layout keywords are reserved nowhere — `top`, `full`,
`include`, `offset`, `end` on MySQL, `partition`, `window`, `include` on SQL Server — so they
are legal unquoted names, and `select * from top` written `FROM TOP` was *Table 'db.TOP'
doesn't exist* on MariaDB 10.11. Where a name's case can matter (`names_may_be_case_sensitive`:
MySQL's tables on Linux, SQL Server under a `_CS_` collation; PostgreSQL folds and SQLite
ignores it) such a word is recased only where it stands as its keyword — `stands_as_name`,
conservative in the name's favour: a name after `FROM`/`JOIN`/`INTO`/`UPDATE`/`TABLE`/`AS`,
and otherwise wherever the keyword's own shape is missing (`FULL` ahead of a join word, `ANY`/
`INCLUDE` ahead of `(`, `TOP` ahead of a count, `PARTITION` ahead of `BY`, `END` with a `CASE`
or body `BEGIN` open, the rest followed by a comma, a close, a comparison or a clause word)
(`keyword_case_leaves_an_unreserved_keyword_used_as_a_name`). The recasing is **ASCII only**, so every byte stays where it was — that is what keeps
`caret_after_format` right over a recased result. `KeywordCase` serialises as a lower-case word
and **deserialises leniently** through a hand-written impl: a word this version does not know
reads as `Keep`, so a value a later version writes never fails the `ui_state.json` it sits in
(`keyword_case_persists_as_a_word`). Deriving `Deserialize` as a tidy-up would make it fail the
whole file. A value that is not a string at all still fails the parse.
**Function case and type case are two more passes of the same kind, run after it** —
`FormatStyle::function_case` and `type_case`, each a `KeywordCase` and `Keep` by default,
applied by `apply_name_case(sql, dialect, functions, types)` (and `apply_name_case_under`,
taking the `DELIMITER` terminator as the keyword-case pair does) over `apply_keyword_case`'s
output in `format_range`. The order is load-bearing: a keyword that is also a function (`LEFT`)
is recased as a keyword first and then takes the function's case where it is called, so `LEFT
JOIN` and `left(s, 3)` can differ (`keyword_function_and_type_case_compose`). **A name has to
pass two questions, because a recased name is a different object wherever names are
case-sensitive.** *Is it the engine's own?* A function is one in this app's catalogue for the
dialect — `intel::is_builtin_function`, a `pub(crate)` wrapper over the private `catalog_index`
and `is_known_function`, so the dialect's whole catalogue, every flavour's names together; a
type is one of `is_builtin_type`'s per-dialect lists, kept in this module. A user's function or
type is never touched, nor anything beside a `.` (`t.lower(c)`, `dbo.money2`). *Does it stand
where it is that?* A function only unqualified, straight before its `(`, and where `is_call`
says it is called: not a layout keyword whose parens are a clause's (`VALUES (`, `IN (`) unless
`is_called_keyword`, not after `INTO`/`TABLE`/`FROM`/`JOIN`/`UPDATE`/`REFERENCES`/`FUNCTION`/
`PROCEDURE`/`VIEW`/`INDEX`/`KEY`/`TRIGGER`/`TYPE`/`AS`/`UNIQUE`/`FULLTEXT`/`SPATIAL`, nor after
an index's `ON` in a `CREATE` — `insert into year(a)` names a table, not `YEAR()`, and a MySQL
table's name may be case-sensitive. Nor where a name stands with no one keyword before it: past
`IF NOT EXISTS`, after an `INSERT`/`REPLACE` that leaves out its `INTO` (over `IGNORE`,
`LOW_PRIORITY`, `TOP (n)` — `inserts_into`), a CTE's name, read off its column list's closing
paren being followed by `AS (` or `AS [NOT] MATERIALIZED`, and on SQL Server a table before a
legacy hint, `log (nolock)` (`is_table_hint`). Each of those was recased once, and each failed
on a server whose names are case-sensitive — measured on MariaDB, MySQL 8.4 and a `CS_AS` SQL
Server database (`function_case_recases_built_in_calls`,
`function_case_leaves_a_name_before_a_paren`). That is conservative on purpose, and `from
json_table(…)` is missed with it, deliberately.
A type only where `type_position` puts one: after `::`; after the `AS` inside a `CAST`/
`TRY_CAST`/`SAFE_CAST` paren; a table body item's second token unless its first is one of
`TABLE_ITEM_WORDS`; a routine parameter's, after its name and any `IN`/`OUT`/`INOUT`/
`VARIADIC`; after `RETURNS`; on SQL Server after an `@var` or `@var AS` **being declared** — in a
`DECLARE`, or a procedure's or function's header before its `AS` (`declares_variables`), never
in a select list, where `@d date` is a column and its alias; and in an `ALTER TABLE` after
`ADD [COLUMN] c`, `MODIFY [COLUMN] c`, T-SQL's `ALTER COLUMN c` (`alter_column_takes_a_type` —
elsewhere `ALTER COLUMN c SET …` is an action, and MySQL's `SET` is also a type), `CHANGE
[COLUMN] old new` and `TYPE`. `COLUMN` itself is never read as the column's name, so `modify
column date int` recases `INT` and not the column. A type's tail words go with it, past a
size's parens — `TYPE_TAILS` (`PRECISION`,
`VARYING`, `UNSIGNED`, `SIGNED`, `ZEROFILL`), and `WITH`/`WITHOUT TIME ZONE` after `TIME` or
`TIMESTAMP` (`type_case_recases_built_in_types_where_they_are_types`). What each open paren
holds is a stack of `NameFrame { holds: ParenHolds, n, first }` — `Table`, `Params`, `Cast` or
`Other`, with the item at its depth counted and its first word kept — the item reset at a comma
and the whole stack at a terminator. `paren_holds` asks the same free `is_table_body_open` that
`Fmt::opens_table_body` does, so the layout and the type case never disagree about which `(` is
a table's body.
**The type list holds only what the engine resolves in any case, and that was measured live
when the setting landed.** On SQL Server 2022 in a `Latin1_General_CS_AS` database, `INT`,
`NVARCHAR`, `DATETIME2`, `UNIQUEIDENTIFIER`, `SQL_VARIANT`, `ROWVERSION`, `SYSNAME`,
`GEOGRAPHY` and `HIERARCHYID` all resolve upper-cased, and `GETDATE`/`getdate`, `ISNULL`,
`STRING_AGG`/`string_agg` and `COALESCE` resolve in either case — but an alias type created as
`mytype` and written `MYTYPE` fails with *Msg 2715, Cannot find data type*. That is why a
user's type is never recased and a qualified name is skipped
(`type_case_leaves_types_it_does_not_ship`): recasing "whatever stands in a type position"
would rename a user's type under exactly the collation where its case is its name. Both passes
are ASCII only, so every byte stays where it was and `caret_after_format` holds over them as it
does over the keyword case.
**Comma placement is the second setting, and it is the layout's, not a pass over it** —
`CommaStyle` is `Trailing` (the default, *End of line*: the formatter as it always was) or
`Leading` (*Start of line*), serialised and read leniently exactly as `KeywordCase` is, an
unknown word being `Trailing` (`comma_style_defaults_to_end_of_line_and_reads_leniently`). The
settings travel as a `FormatStyle { case, commas, gap, join_on_own_line, river, align,
tab_width, wrap, function_case, type_case }`, and `format_range`/`format_statement`
take `impl Into<FormatStyle>`, with `From<KeywordCase>` putting every other setting at its
default — so a caller or test that passes a bare `KeywordCase` says nothing about the rest and
is unchanged. `format_sql` stays at the defaults. `format_sql_under` takes the whole
`FormatStyle` and sets the commas into `Fmt::leading_commas`,
and the `","` arm moves only a **breakable** comma — the rule the layout already broke at: top
level, or a subquery's or a listed paren's list (a table body, or a list `wrap` broke) — taking
the same `break_to(self.content)`
*before* the comma instead of after it. Every other comma stays where it was, so `IN (1, 2)`
and a function's arguments are inline either way; the test states that as a property against
the default output — each line-ending comma becomes the next line's leading `, ` and nothing
else differs (`leading_commas_move_the_breaking_commas_and_no_others`). A line comment after a
broken comma still ends its line, never with the next column on it
(`a_leading_comma_never_puts_code_after_a_line_comment`). The comma stays between the same two
tokens, so only whitespace moves and `caret_after_format` holds over it as it does over a
trailing layout.
**The blank lines between statements are the third** — `StatementGap`, read leniently as the
other two are, an unknown word being `One` (`statement_gap_reads_leniently`). `One` (the
default, *One blank line*) is the formatter as it always was: one blank line after a statement
at the margin, none between a body's, whatever the input had
(`one_blank_line_between_statements_is_the_default`); `None` (*No blank line*) runs every
statement into the next (`no_blank_line_runs_the_statements_together`). `Keep` (*As typed*)
writes one blank line wherever the input had one or more and none where it had none, **at any
depth**, so a body's statements may be parted too, which the other two never do. Both live in
`end_statement`: `One` sets `blank` only at level 0, and `Keep` sets `Fmt::gap_pending`
instead, which stays open across the comments between the two statements and is settled at
the next one's first code token. **Each token on a new line up to there sets `blank` from the
newlines the input had before it** — a comment, line or block, with no newline before it
trailed the terminator and is still the statement's line, so `select 1; -- one⏎⏎select 2;` and
`select 1; /* done */⏎⏎select 2;` keep their gap after the comment, and a blank after a
comment on its own line (`select 1;⏎-- part one⏎⏎select 2;`) stays after it rather than being
lost to the comment's own blank-free run-up (`as_typed_keeps_the_blank_lines_the_input_had`,
which holds those, a run of comments and a procedure body). Several blank lines become one;
the setting does not keep counts. A `GO` line is outside it: the batch after one starts after a blank line under all
three, as it always did.
**A join's condition on its own line is the fourth**, and the first that is a plain `bool` —
`FormatStyle::join_on_own_line`, off by default, which is the formatter as it always was:
`join u on u.id = t.u_id` on one line, its `AND`/`OR` breaking back to the join's own indent.
On, the `ON` breaks to `content + 1`, one in from the join, and the condition's `AND`/`OR`
follow it there while `Fmt::in_on` is set; a second format moves nothing
(`a_joins_on_can_take_its_own_line`). **Only a join's `ON` moves, and that is the state's whole
job**: `ON` is also an index's `ON t`, a trigger's `ON t`, a key's `ON DELETE` and a `MERGE`'s
`ON`, none of which is a join condition. So `Fmt::joined` is set only where a join keyword
breaks (`is_join_break`) and cleared by a clause head, in `end_statement` **and by the `ON` it
takes — one join owns one `ON`** — and the `ON` arm asks for it, and refuses an `ON` before
`DUPLICATE` or `CONFLICT`. A test of the word alone would break every one of those
(`an_on_that_is_no_joins_keeps_its_line`, which holds all four — a `MERGE`'s keeps its line
because no join keyword precedes its `ON` — and an upsert's `ON DUPLICATE KEY`/`ON CONFLICT`
after an `INSERT … SELECT`'s join, which no head separates from it). **A subquery
has joins of its own**: a subquery `(` saves both flags in `Paren::saved_joined`/`saved_on`
and clears them, and its `)` restores them, so the inner join's `ON` and the outer one's after
`) x` each answer for their own join. What was one branch for a join keyword and `AND`/`OR`
together is three — the join break, the `ON`, the `AND`/`OR` — since the last now asks which
indent it breaks to.
**The river is the fifth**, a `bool` too — `FormatStyle::river`, off by default. On, each clause
keyword is right-aligned on one edge and its body goes on the keyword's own line, so the bodies
share a column: `select a,⏎       b⏎  from t⏎ where x = 1⏎   and y = 2`. The edge is
`const RIVER: usize = 6`, `SELECT`'s width and that of every head of a query's first line; a
longer head such as `RETURNING` takes no pad and runs past the edge. All of
it is in `Fmt::step`. A head breaks to `Fmt::river_at(word)` — `base`, padded in `Ind::col` so
the word ends on the edge — save at a statement's start (`Fmt::prev_code` is `None`, so no
pad: the last token that is no comment, so a comment above `WITH` or `SET` does not push it
onto the edge, `a_comment_before_a_statement_changes_nothing_after_it`) and at a
subquery's first head, which stays on its paren's line (`Fmt::river_inline_head`, **cleared by
every `emit`**, so it answers only for the token straight after the `(`). There is no break
after the head: `content` becomes the column just past the head and its followers
(`Fmt::here()`, whose `line_cols()` counts from `Fmt::line_body_start`, set where `emit` writes
a line's indent), so a list continues under its first item wherever the head ended —
`group by a,⏎          b`. A head with a follower aligns its *first* word, and the `BY` runs
past the edge. Join keywords, a join's own-line `ON` and `AND`/`OR` break to `river_at` of their
own word, so a `left join` puts `left` on the edge (`a_joins_on_in_the_river_joins_the_edge`).
**A subquery's `(` does not break**: its `base` is the paren's column less the pad its first
keyword will have, so the subquery's own river lines up under that keyword, and its `)` closes
on its last line instead of on a line of its own (`a_subquery_keeps_its_own_river`). Two
things the block layout took from `line_indent` read differently here, because under the river a
line's indent is its keyword's pad, not where its body starts: a laid-out `CASE`'s frame indent
is the `CASE` word's own column (`here()` less the word), so `WHEN`/`ELSE` hang one unit in from
it and `END` returns to it (`a_case_in_the_river_hangs_from_its_case`); and a line comment's
fallback break is `content`, the body's column, where it is `line_indent` otherwise. With a tab
unit the levels are tabs and the river's alignment is spaces after them
(`the_river_aligns_with_spaces_after_the_indent`). Only whitespace moves, held against the
default layout's tokens (`the_river_moves_only_whitespace`); every river test also formats its
output again through a `settled` helper that asserts nothing moves.
**Aligned columns are the sixth** — `FormatStyle::align`, a `bool`, off by default
(`alignment_is_off_by_default`). On, a broken list's items line up on what they share: a select
list's `AS` (the clause is `SELECT`, the word is not in an expression paren and not after a
`.`, where it is a name), an `UPDATE`'s assignment `=` (the clause is `SET`, not in an
expression paren — so a `WHERE`'s comparison never moves), and a table body's column type, the
item's second token, unless the item's first word is a key's or a constraint's
(`TABLE_ITEM_WORDS`: `PRIMARY`, `UNIQUE`, `KEY`, `INDEX`, `CONSTRAINT`, `FOREIGN`, `CHECK`,
`FULLTEXT`, `SPATIAL`, `EXCLUDE`, `PERIOD`, `LIKE`). An item with no such token is left as it
is (`a_select_lists_aliases_line_up`, block and river; `assignments_and_column_types_line_up`).
**It is a pass over the finished text, not the layout's `Ind::col`** — the pad goes in
mid-line, ahead of the marked token, and a group's widest column is known only once its last
item is written. During layout `step` sets `Fmt::mark_next` for such a token, and `emit`
records a mark, `(group, offset in out)`, only if the item has none yet (`Item::marked`) and
the token is on the item's **first line** (`Item::line` against `line_body_start`): a column
on an item's later line lines up with nothing, and `) as sub` after a subquery laid out over
several lines would otherwise be padded out to the aliases above it. `Item::fresh` is set by
`Fmt::new_list` and after every breakable comma, under either comma style, and the next `emit`
**of code** starts the item — its line, and its first word upper-cased. A comment neither
starts an item nor counts among its tokens: the `-- the key` trailing `id int,` is written on
the comma's line, and taken as the next item's first token it put that item's line a line too
early, so the column after every commented one never lined up
(`a_trailing_comment_does_not_take_the_next_items_place`). **Each list is a group of its
own**: `new_list(clause)` hands out a new one at every clause head (the clause being the head
word) and when a subquery's or a table body's paren opens (no clause), and
`Paren::saved_align` keeps the outer `(group, clause, item)` for its `)` to restore — so a
subquery's aliases line up among themselves and the outer list's go on around it
(`each_list_lines_up_on_its_own`). A list `wrap` broke opens no group but saves and restores
the same state, because its own commas are breakable and each sets `Item::fresh` — without the
restore they would end the item the list sits in, the `coalesce(…)` whose `as n` is to line up
(`a_wrapped_list_does_not_disturb_alignment`). The column-type mark asks
`Paren::table`, not `Paren::listed`: `listed` means only "one item a line" now, and a wrapped
call's second token is no column type. Then `Fmt::apply_alignment`, which `format_sql_under` calls
after `run`, measures every mark's column from its line's start — a tab as `tab_width` — and
pads each mark of a group with two or more to the group's widest, inserting from the end back
so no offset still to pad moves. A leading comma is part of its line, so `, bbbb as y` still
shares the column (`aliases_line_up_after_leading_commas`). Only spaces go in, so
`caret_after_format` holds over it. `tab_width` `0` reads as 4: `theme::format_style()` fills
in the editor's tab width each time it is read, and the app's composing effect sets `0`.
**The wrap width is the seventh** — `FormatStyle::wrap`, a `usize`, `0` meaning off and the
default (`wrapping_is_off_by_default`). A list the layout keeps on one line — a call's
arguments, an `IN` list, a `VALUES` tuple: any expression paren with a comma at its own depth —
goes one item a line once its line would run past the width
(`a_list_that_overflows_the_width_goes_one_item_a_line`). The `(` arm asks `Fmt::overflows`
for a paren that is neither a subquery nor a table body. It finds the matching `)` — giving up
at a terminator, a directive or a `;` — and wants a comma at depth 1, so a paren with no list
in it never breaks however long, there being nothing to break it at. Then it **measures by
doing**, which is what the `step` seam is for: it takes `out` and `marks` aside, clones the
`Fmt` and puts them back — so the clone is the layout state, not the text written so far —
sets `Fmt::measuring`, steps from the `(` to its `)`, and asks whether any line from the `(`'s
line on is wider than `wrap`, a tab counted as `tab_width` — the line it *lands* on: where a
break is pending the `(` opens a new line, and measuring from the last `\n` written took in the
long line above it, so a short `VALUES` row under a long comment went one item a line
(`a_list_on_its_own_line_is_measured_on_it`); then it truncates `out` and
`marks` to where they were and restores the clone. So the width it measures is the one it
would write, spacing and case frames and all, rather than an estimate of it. While `measuring`
is set `overflows` answers `false`, so the lists inside stay on one line for the measure; each
is asked again when its own `(` comes round, and only the lists that overflow break —
`f(g(a, b), long_one, long_two)` puts `g(a, b)` whole on its new line
(`only_the_overflowing_list_breaks`, which holds the comma-less paren too). A list that
overflows becomes a listed paren as a table body is: `Paren::listed` holds its anchor, the
items go at anchor + 1 and the `)` back at the anchor. The anchor is `line_indent`, or under
the river `content` — the body's column, a river line's own indent being its keyword's pad
(`a_list_wraps_from_the_rivers_body`). `Paren::table` is what tells a table body from a
wrapped list now (above).
**`formattable_range` is what decides how much of the document a Format may touch**, and it is
here rather than at the call site because `format_sql` lexes what it is handed **from byte 0**
and takes that for a real token boundary. True of a whole document and of a selection whose ends
are in code; false of one that begins inside a string, a quoted identifier or a comment. With
`select 1 -- keep a, b\nfrom t` and `keep a, b` selected, the fragment lexed as the words `keep`,
`a`, a comma and `b` — the comma broke, and the document became
`select 1 -- keep a,\nb\nfrom t` with `b` now *outside* the comment as a stray token, so the
statement no longer parsed. An empty selection is the whole document, which is what a bare caret
has always meant; a selection that is not formattable answers `None` rather than falling back to
the document, because silently reformatting every line the user did not select is the worse of
the two surprises. Both ends are asked through `pairs::region_at`, so the two halves of "is this
code" stay one answer.
**What a Format writes over that range is `format_range`'s, not `format_sql`'s**, because
`format_sql` trims both ends and a selection's outer whitespace belongs to the document around
it: `select a from ` selected out of `select a from t` — where a Ctrl+Shift+→ selection ends,
after the space — came back ending in `from`, and the splice fused it with `t` into `fromt`. The
range's own leading and trailing whitespace is put back unless the range is the whole document,
which stays `format_sql`'s, trailing-newline rule and all
(`formatting_a_range_keeps_the_whitespace_around_it`). `caret_after_format` is where the caret
goes afterwards — a Format of the whole document used to leave it at the end — and it counts the
non-whitespace bytes before the caret and finds as many in the result, a caret in whitespace
landing straight after the token before it (`the_caret_keeps_its_place_through_a_format`). That
leans on this module's contract that only whitespace moves: a layout that ever changed a token's
text would put the caret somewhere else. Keyword, function and type case are the only
sanctioned changes to a token's text, and they survive that only because they are ASCII and
leave every token's length alone.
**`single_line` is the editor's *Copy as single line*, and it is not `format_sql` with the
breaks left out, because a line break is not always layout.** A whitespace run in code becomes
one space and a block comment's runs collapse too, but a `--` or `#` comment ends at its line
and would swallow the rest of the statement, so it is rewritten as `/* … */` (an empty one is
dropped) — and the whole copy is refused, `None`, unless `sql::is_one_comment` reads the result
back as one comment closed at its own end: a `*/` in the text would close it early, and where
comments nest a `/*` would leave it open. A MySQL executable comment is the one that check
cannot see: `skip_noncode` hands back its marker (`/*!`, `/*!nnnnn`, `/*M!…`) as a span of its
own and lexes the body as code, so a `--` inside `/*!50001 … */` was rewritten `/* n */`, whose
`*/` closed the executable comment early (`/*!50001 SELECT 1 /* n */ */`). Once a marker has
been seen, a non-empty line comment refuses the copy — even after the executable comment has
closed, since the walk does not track its end.
Strings, quoted names and dollar-quoted bodies are
copied **verbatim, line breaks included** — they are data, and a copy that changed them would
not be the statement it was copied from — so a PL/pgSQL body keeps its lines and its `--`
comments. A `GO` or `DELIMITER` line refuses it too (`sql::go_directive`/`delimiter_directive`,
asked at the first non-blank code byte of each line), since a client reads either only standing
on a line of its own. That is deliberately conservative: unlike `tokenize`, it does not ask
whether a statement begins there, so a line that merely opens with a word `delimiter`
mid-statement refuses as well (`single_line_refuses_what_one_line_would_change`). **So does a
string continued on the next line where a line break joins two strings into one**
(`newline_continues_strings`, PostgreSQL's alone): `'foo'⏎'bar'` is `'foobar'` there and
`'foo' 'bar'` a syntax error, which *Make single line* wrote over the statement. The break may
pass through blanks and line comments but not a block comment, as PostgreSQL's lexer has it;
MySQL joins adjacent strings on one line too, so there the copy stands
(`single_line_refuses_a_postgres_string_continued_on_the_next_line`).
`single_line_copy` picks what is copied — the selection when there is one, else
`sql::statement_range` at the right-click, trimmed, an empty answer being `None` — and puts the
selection through `formattable_range` for that function's reason: `single_line` lexes from byte
0 as well, and a fragment opening inside a string takes its closing quote for an opening one
(`single_line_copy_takes_the_selection_or_the_statement_at_the_click`). That choice is
`single_line_target`'s, which **`single_line_in_place`** shares: the same text with the range
it replaces, for *Make single line*, and `None` as well when the text is one line already, since
that edit would change nothing. The range is the trimmed one, so the line breaks either side of
the statement stay its neighbours' (`single_line_in_place_rewrites_only_the_statement_or_selection`).
**`format_statement`** is *Format statement*'s answer in the same shape: the
`sql::statement_range` at the caret through `format_range`, or `None` for blank text or a
statement formatted already (`format_statement_formats_only_the_statement_at_the_caret`).
"Formatted already" includes the keyword, function and type case: both take the `FormatStyle`,
so a statement laid out already but cased otherwise is still `Some` — and still offered by the editor's menu
(`formatting_writes_the_keyword_case`).
**`caret_after_replace`** places a bare caret after either rewrite — unmoved before the range,
shifted by the length change after it, `caret_after_format` within it — and is approximate inside
a single-line rewrite, where a `--` comment gains the `/*` and `*/` it becomes and a caret past it
lands a few bytes early.
**`tokenize` takes a numeric literal in one arm, ahead of the word arm, because two of a
number's three parts are spelled in bytes that arm calls punctuation.** `1.5e-3` lexed as
`["1", ".", "5e", "-", "3"]`; `need_space` suppresses the space around a `.`, but nothing said
anything about a `-` after a word, so the `"-" | "+"` arm asked `is_keyword("5e")`, got `false`,
called the sign binary and spaced it — and Format Code wrote `1.5e - 3` back over the user's own
buffer. `1.5e` is not a number on any of the three engines: SQLite lexes it `TK_ILLEGAL`,
PostgreSQL answers *trailing junk after numeric literal*, MySQL reads `1.5` and an identifier
`e`. The arm takes only the shape all three lexers share and no more of it — a `.` only ahead of
a digit or of an exponent, and a sign only ahead of a digit *and* behind the `e` the word scan
already swallowed — so `select a-1` keeps its binary minus and `select 1 e` keeps its two tokens.
The exponent half came later: `1.e-5` is a number on all four engines, and asking only for a
digit after the `.` wrote it back as `1.e - 5`
(`a_number_with_a_bare_point_before_its_exponent_is_one_token`). So did a number that begins or
ends at its point, `.5` and `1.`, valid on all four: lexed as a `.` beside a word, `need_space`
kept the point tight and wrote `between .1` as `between.1` (MariaDB: ERROR 1064) and `1. as x`
as `1.as x` (PostgreSQL: *trailing junk after numeric literal*). The arm now starts at a `.`
ahead of a digit unless a name, a quote, a `)` or another `.` stands straight before it — a
qualifier's point, as the engines read it — and takes a trailing `.` after an all-digit integer
that no word byte or second `.` follows and that is no qualified name's part
(`a_number_with_a_leading_or_trailing_point_keeps_its_spaces`).
**Two more ways it broke a token's *text*, against its own contract that only whitespace and
layout change.** It was a fourth caller of `skip_comment` and took the security gates' answer, so
a `/*! … */` body became ordinary tokens and the closing `*/` fell out as two one-byte puncts
that `need_space` separated: `/*!40101 SET NAMES utf8 */;` reformatted to an **unterminated
comment** that swallows the rest of the file, and idempotently, a second Format seeing the same
shape. `tokenize` asks `sql::executable_comment_end` now, ahead of `skip_noncode`, and takes the
whole run as one verbatim slice; a `*/` with no opener in the slice — what a selection ending
past the close of one hands it — is one token for the same reason, `ops(MySql)` carrying no `*/`
entry. The other was the `:name` arm, which copied half of `params`' rule: asking only "is the
next byte an identifier byte" made `a[lo:hi]` one token, and `need_space` wrote it back as
`lo :hi` — a spelling `params` *does* read as a placeholder, so Format Code created a query
parameter by moving a space and held the run until it was bound. That rule has one definition
now, `params::opens_placeholder`, and both callers ask it.
**A T-SQL `GO` line is a `Kind::Directive`, one verbatim token that owns its line.** It is the
client's batch separator, not SQL, and only on a line of its own; `tokenize` read it as an
ordinary word, so Format Code laid it out on the line before (`SELECT 1 GO`), where it separates
nothing, and the script lost its batches. `tokenize` asks the splitter's own definition —
`sql::go_directive` at `sql::at_line_start`, `pub(crate)` for this — and takes the line whole,
count and `--` comment included; `run` puts it at the margin on a line of its own and resets the
layout state as a `;` does, the next batch starting after a blank line. `go_directive` answers
`None` on every other dialect, so there `go` is still a word
(`a_sql_server_go_line_survives_formatting`, which asserts the formatted script splits into the
original's statements, each formatted). **The converse holds there too: a *name* `go` is never
left alone at the start of a line**, since on SQL Server that line is the directive.
`select a, go⏎from t` came out with `go` on a line of its own, splitting the script into two
broken batches, and a second Format read it as one, so formatting was not idempotent. On a
`batch_separator` dialect (`go_separates`) `run` cancels the break pending before a word `go`
that does not follow a `.`, keeping it on the line before; where a line comment forced that
break, `go` starts the line and `glue_next` pulls the token after it onto the same one
(`a_name_spelled_go_never_becomes_a_batch_separator`, which also formats its output again).
**MySQL's `DELIMITER` line is the same kind of token**,
and was read as SQL: `DELIMITER $$` came out `DELIMITER $ $` joined to the `CREATE` after it,
`END$$` and the `DELIMITER ;` line ran together, and `//` split into `/ /` — a script no client
could split. `tokenize` asks `sql::delimiter_directive` (`pub(crate)` for this) only where a
statement begins (`at_statement_start`: after the terminator in force, a directive or nothing,
comments aside — where `statement_ranges` takes one), keeps the line verbatim, and while the
terminator it set is not `;` reads it as one `Kind::Terminator`, which a word stops at (`END$$`)
and `run` treats as `;` (`end_statement`), written tight unless spelled in word bytes
(`a_mysql_delimiter_line_survives_formatting`, by the same split-and-compare, and idempotent).
**A fragment is lexed under the terminator in force where it begins**: *Format statement* and a
selection hand `format_range` a routine's `END//` without the `DELIMITER //` line above it, so
the `//` lexed as two `/`s and came back `END / /` (and `;;` as two `;`s) — no terminator, and
the client ran the routine on through `DELIMITER ;`. `terminator_at` tokenizes the document up
to the fragment and hands the state it ends in to `tokenize` (whose `delim` is that state, `None` for a whole
document), through `format_sql_under`
and `apply_keyword_case_under`, so the directive is taken exactly where `tokenize` takes one
(`a_fragment_under_a_delimiter_keeps_its_terminator`).
**Idempotence is held as a property over the widest corpus the crate has**: `intel`'s
`valid_corpus`, formatted on all four engines, and `format_corpus` — the shapes Format Code
has got wrong, each on its own engine — must format to itself a second time and **say the same
thing**: the same statements as `sql::executable_statements` splits them, each the same
`sqlparser` tokens with the blanks and comments dropped (`what_it_says`) — save the line break
that joins two strings where `newline_continues_strings`, kept as a token of its own. The
*Format statement* of each statement alone, spliced back, and `single_line` wherever it does
not refuse are held to the same (`the_corpus_formats_idempotently_on_every_engine`,
under `intel`'s tests for where the corpus lives). **Each layout setting is a second layout and
is held to the same two properties** over the same corpus and engines —
`the_corpus_formats_idempotently_under_every_layout_setting`, for a list `styles` of non-default
`FormatStyle`s (the river; the river with `join_on_own_line`, `StatementGap::Keep` and leading
commas; `join_on_own_line` with `StatementGap::None` and `align`; the river with `align` and
`wrap` 40; the block layout with `wrap` 30 and leading commas) —
never a keyword, function or type case, each of which changes a
token's text by design — where a second format must move nothing and
`what_it_says` must not change. The formatter's own tests pin a setting a shape at a time, and
this is where a shape none of them thought of shows up. **A new layout setting joins
`styles`**; a setting left off it is held to neither property here. It compared non-blank characters until
then, which is blind to every split and fusion this formatter can make — `END//` and
`END / /`, `between .1` and `between.1`, `- ~5` and `-~ 5` have the same ink — and a census
taken with `sqlfmt::tokenize` itself agrees with itself about a token it mis-splits, so the
reader is deliberately another lexer. Any other blank that matters *between* two tokens it
cannot see, and the module's own tests pin those. The token-preservation test runs its shared shapes
on all four engines too, and SQL Server's own besides — bracketed names with `]]`, `N'…'`,
`@x`/`@@x`/`#t`/`##t`, compound assignment, `TOP (n)`, `OUTPUT`, `APPLY`, `type::Method`,
`!<`/`!>` — each idempotent. Both were green on arrival: they guard, they did not find.
**A line comment that trailed its line's code stays on that line**: with no newline before it in
the input, the break the code before it asked for — a list comma's, a clause head's, a `;`'s —
is taken after it instead (`deferred` in `step`'s `LineComment` arm). Moved down past the break,
`a, -- first` came out over `b` and read as `b`'s note. `need_space` sets a line comment off
from the code before it first thing, since after a `;` `end_statement` has cleared `prev` and
`1;-- done` came out tight (`a_trailing_comment_stays_with_the_code_it_followed`).
**A `CASE` is laid out** (`cases`, one `CaseFrame` per open `CASE`): each `WHEN` and the
`ELSE` one level under the line the `CASE` opened on, `END` back at that line's level, so
nested ones nest. A frame belongs to the paren depth it opened at, so only its own
`WHEN`/`ELSE`/`END` are its; one opened inside a function's parens stays inline; and a routine's
`END CASE` closes a frame rather than opening one — on MySQL, where the statement is a body
instead (below), it closes that (`a_case_lays_out_its_branches`). `WITH
RECURSIVE` keeps its word on the head's line (`head_followers`), `WINDOW` is a clause head, and
MySQL's `LIMIT offset, count` keeps its comma inline — `limit_depth` holds the paren depth the
`LIMIT` opened at, so a comma after a subquery's `LIMIT` breaks as the list's again
(`recursive_window_and_limit_lay_out_as_clauses`). T-SQL's `TOP (n)` keeps its space (`TOP`
is in `is_keyword`, so its paren is a clause's, not a call's), and a write's `OUTPUT` is a head
— only where `has_output_clause` (SQL Server), only in an `INSERT`/`UPDATE`/`DELETE`/`MERGE`
(`Fmt::verb`, the statement's first *code* word — read where `Fmt::prev_code` is `None`, so a
comment above the statement no longer hides it, and with it a `CREATE TABLE`'s body, which
went unlisted, `a_comment_before_a_statement_changes_nothing_after_it`; that also covers
`at_statement_start`, which opens MySQL's compound statements), and only with a name after
it, since a procedure parameter's `@x int OUTPUT` takes no parens and a column may be called
`output` (`top_keeps_its_space_and_output_is_a_clause`). **A `BEGIN … END` body is indented**
(`Fmt::blocks`, a stack of the word that opened each open body): `end_statement` returns a
body's `;` to the body's level, not the margin, and with no blank line between its statements
unless `StatementGap::Keep` finds one in the input;
`END` steps back to its `BEGIN`'s; T-SQL's `TRY`/`CATCH` stay with their word
(`emit_block_follower`); a `BEGIN` takes a line of its own unless it ends a header (`AS BEGIN`,
`THEN BEGIN`) or follows a label (`lbl: begin`). A transaction's `BEGIN` — `TRAN`,
`TRANSACTION`, `WORK`, `DISTRIBUTED`, or a bare `BEGIN;` — opens nothing (`opens_body`), and a
`GO` ends whatever its batch left open (`a_begin_end_body_is_indented`). **On MySQL a compound
statement is a body too**, where `pairs::names_its_block_ends` says so — the same predicate
the editor's block matching asks. `IF`, `WHILE` and MariaDB's `FOR` at a statement start
(`at_statement_start`: nothing yet, or a label's `:`) write their header on the line, its
content one deeper so an `AND` in the condition breaks one level in, and `Fmt::awaiting` holds
the word that ends it — `THEN`, `DO` — which opens the body (`open_block`); `LOOP` and
`REPEAT` open theirs at once. An `ELSEIF`/`ELSE` goes back to its `IF`'s level and reopens;
`UNTIL` goes back to its `REPEAT`'s, and the `END REPEAT` after it stays on its line
(`until i > 5 end repeat;`). **A `CASE` statement is laid out as an `IF` is**, not through
`cases`: a `CASE` at a statement start, outside an expression, pushes `"CASE"` onto `blocks`
without breaking the line, so its operand stays on it (`case x`). A `WHEN` or `ELSE` with no
expression `CASE` open here (`case_here`) while `blocks` ends in `"CASE"` or `"WHEN"` pops the
previous branch's `"WHEN"` and is written one level in from the `CASE`; a `WHEN` awaits its
`THEN`, which opens a `"WHEN"` body, and an `ELSE` opens one at once — so each branch's
statements sit one level further in. Through a `CaseFrame` the branches came out flat at the
enclosing body's level, since the `;` ending a branch's first statement cleared the frame. A
`CASE` *expression* — anywhere else, a branch's own statements included — still takes the
`cases` path (`a_case_statement_lays_out_its_branches`, a searched `CASE` holding a `CASE`
expression among its shapes, and idempotent). Which `END` closes which body is
`closes_block`: a bare `END` (or `END TRY`/`CATCH`) a `BEGIN`, an `END IF`/`LOOP`/`WHILE`/`FOR`
its own kind, `END REPEAT` the `UNTIL`, and an `END CASE` a top of `"WHEN"` (its last branch)
or `"CASE"` (none) — popping the `"WHEN"` and then the `"CASE"` beneath it, back at the
`CASE`'s level. `IF (…)` is the statement only when `THEN`
follows the parens (`if_statement`), so `IF(a, 1, 2)` stays a call, and the statement's
`IF (a > 1) THEN` keeps its space. A label's colon is written tight before
`BEGIN`/`LOOP`/`WHILE`/`REPEAT`/`FOR` on every engine — it came out `l : loop`. Where
`names_its_block_ends` is `false` an `END IF` and its kin still close no body: T-SQL's `IF`
has no `END IF` and opens nothing (`a_compound_statement_body_is_indented`).
**A T-SQL `@x`, `@@ROWCOUNT`, `#t` or `##g` is one `Kind::Word`.** `@` and `#` fell to the
operator arm and `need_space` separated them from their name, so one Format Code broke nearly
every T-SQL script: `SELECT @ x` is Msg 137, `SELECT @ @ VERSION` and `CREATE TABLE # t` are
Msg 102 (measured on 2022; review finding S1.1-L1-02). The word arm takes the prefix
`sql::t_sql_name_prefix` measures — `0` unless a word byte follows it, so `a @ b` is left alone,
and `0` on every engine but SQL Server — and continues every word with `sql::continues_name`,
so a name that *holds* one of those bytes, `h#` or `f@x`, stays whole too
(`a_t_sql_variable_or_temp_table_keeps_its_prefix`), and so does `a$b` on every engine
(`a_dollar_inside_a_name_keeps_the_name_whole`).
**Every engine's own sigil is kept the same way** (`sigil_len`, of which the T-SQL prefix is
one arm). The other engines' were split from their names: PostgreSQL's `$1` came out `$ 1`,
SQLite's `?1`, `$p` and `@p` came out `? 1`, `$ p` and `@ p`, and MySQL's `@x`, `@@version` and
an account's host, `'u'@'h'`, came out `@ x`, `@ @ version` and `'u' @ 'h'` — each refused
where the input ran (MySQL 8 and MariaDB 10.11 measured; `'u' @'h'` and `u @h`, the spaced forms
the fix wrote, both run — measured again on 2026-10-05, after an audit took them for
refused: the space *after* the `@` is the one MySQL 8 refuses). A quoted user's host is now
written tight, `'u'@'h'` and `` `u`@`h` `` (`need_space`; a view's `DEFINER` pins the backtick
form in `a_views_header_keeps_its_line_and_its_check_option_takes_one`); a bare `u @h` is still
spaced.
So `$` before a word byte is a
prefix on all four (a dollar *quote* is `skip_noncode`'s and taken first; MySQL's `$p` is a
name, T-SQL's `$1.50` money and `$action` `MERGE`'s), `?` before a digit on SQLite, and
`@`/`@@` on MySQL and SQLite, MySQL's also before a quote, so `@'q v'` is one `Kind::Quoted`.
PostgreSQL's `@` is an operator and is left alone
(`a_native_parameter_or_variable_keeps_its_sigil`).
**What the input had between two tokens** is read where the layout depends on it: the token
stream does not carry it, so `Fmt` holds `src` and `between` slices the gap out of it, every
token being a slice of `src`.
**A `[` is an array constructor's or a subscript's, and `Fmt::brackets` counts the open ones**:
a comma inside them is an element list's and never breaks a clause list, the brackets sit tight
against what they follow and what they hold, and a slice's `:` binds both bounds — `ARRAY[1,2]`
came out `array [ 1,⏎  2 ]` (`brackets_are_neither_spaced_nor_broken`). A bracket-quoted name is
a `Quoted` token on the engines that have one, so this never sees it.
**Some words are laid out by what is around them rather than what they spell.** A word straight
after `.` is a plain name, never a clause head, a join or an `AND`/`OR` break: MySQL reads a
keyword as a name only when nothing separates it from the `.`, so `select t.order, p.left` came
out `t.⏎order` — `ORDER` the keyword again (`a_word_after_a_dot_is_laid_out_as_a_name`).
`LEFT`, `RIGHT`, `REPLACE` and `INSERT` with a `(` straight after them are calls, a plain word
with a tight `(` (`is_called_keyword`); laid out as the clause or join they also spell, these
everyday string functions broke the select list. None of those clauses is ever followed by `(`
(`a_keyword_called_as_a_function_stays_a_call`). **A head word that only spells a clause
inside something else stays on its line** (`only_spells_a_clause`): `DELETE`/`UPDATE`/`INSERT`
after `ON`, `FOR`, `BEFORE`, `AFTER`, `OF`, `OR` or `INSTEAD` (a foreign key's action, a row
lock, a trigger's event); `SET` after `CHARACTER` or an action's `DELETE`/`UPDATE`, and every
`SET` of an `ALTER`; `REPLACE` of `CREATE OR REPLACE`; and a `WITH` that opens no CTE list —
one not followed by `RECURSIVE` or by a name and then `AS` or a column list, so `WITH
ROLLUP`, `WITH CHECK OPTION` and `TIMESTAMP WITH TIME ZONE`. `ON DELETE CASCADE` came out
`ON` / `DELETE` / `CASCADE` on three lines, `DELETE` laid out as a statement
(`a_word_that_only_spells_a_clause_does_not_open_one`). **A `CREATE TABLE`'s body is one item a
line** (`opens_table_body`: `CREATE [TEMPORARY] TABLE [IF NOT EXISTS] name (`, over the free
`is_table_body_open` that the type case's `paren_holds` shares): its `(` is set
off from the name (`spaced_paren`), each top-level comma breaks, and its `)` returns to the
opener's indent — a `Paren::listed`, whose inside is still an expression paren, so `CHARACTER
SET` and `ON DELETE` keep their lines; `KEY`, `UNIQUE` and `CHECK` take a spaced `(` as a
clause's does (`a_create_tables_body_is_one_item_a_line`). **A column list after a name is
set off the same way** (`opens_column_list`): `REFERENCES u (id)`, `INDEX ix (a)`, `KEY k (a)`,
a view's `VIEW v (x, y)`, and — only in a `CREATE … INDEX` statement — its `ON t (a)` and the
method's `USING btree (…)`, so a join's `ON f(x)` stays a call; `INCLUDE (x)` is a keyword's
paren (`an_indexs_column_list_is_set_off_from_its_table`). **An `ALTER TABLE` with several
actions puts one on a line**, one in from `ALTER TABLE [IF EXISTS] [ONLY] name`
(`opens_alter_actions`, which looks ahead for a top-level comma before the statement ends); a
single action keeps the header's line, and a comma inside its own parens — `ADD INDEX ix (a,
b)` — is not a second (`an_alter_tables_actions_go_one_a_line`,
`a_single_alter_action_keeps_its_line`). The `OR` of `CREATE OR REPLACE` broke the line as a
boolean `OR` and no longer does, and SQLite's `INSERT OR IGNORE INTO`/`UPDATE OR ROLLBACK` and
MySQL's `INSERT IGNORE INTO` keep their conflict words on the head's line (`head_followers`;
`an_or_that_spells_a_statement_is_no_boolean`). A view's `WITH [CASCADED | LOCAL] CHECK OPTION`
takes a line of its own at the statement's base, ending the query
(`a_views_header_keeps_its_line_and_its_check_option_takes_one`). Like everything here, these
move only whitespace — no token's text changes. And a `'…'` literal that
followed another across a newline in the input breaks to a new line again, through `between`:
on PostgreSQL `'foo'⏎'bar'` is one literal only because of the newline, and joined onto one line
it is a syntax error. Literals written on one line stay on one
(`string_literals_continued_across_a_line_stay_on_two_lines`).
