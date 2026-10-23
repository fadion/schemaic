# `core/sql.rs`

`sql.rs` — one `skip_noncode` tokenizer → statement splitting, unsafe-statement guard, AI
read-only gate, `edit_distance`. The *single* SQL boundary lexer; `intel` (scope/context/
diagnostics)/`sql_highlight`/`sqlfmt` all build on it so string/`#`/`--`/`/* */`/backtick
boundaries agree by construction.
**The per-dialect rules are a capability table on `SqlDialect`**, one predicate per divergence
(`dash_comment_needs_space`, `hash_line_comment`, `backslash_escapes`, `e_string_backslash`,
`double_quote_is_ident`, `backtick_ident`, `bracket_ident`, `bracket_doubles`, `dollar_quoted`,
`nested_block_comments`, `delimiter_directive`, `batch_separator`), and they are predicates
because the question stopped being binary. SQL Server filled in the table as a fourth row:
`[…]` names (shared with SQLite, but only T-SQL writes a `]` inside one as `]]` —
`bracket_doubles`; SQLite's span ends at the first `]`, and ending T-SQL's there too would cut
`SELECT [a]]; DROP TABLE t; --] FROM t` into statements the server reads as one name —
`a_doubled_bracket_stays_inside_a_sql_server_name`), comments that nest (shared with
PostgreSQL), and `batch_separator`, which is two rules. A `GO` line — `go_directive`, alone on its
line with an optional count and `--` comment — ends a batch and is the client's, never sent, so
`is_delimiter_directive` answers for it beside MySQL's `DELIMITER` and one predicate lets every
executing path drop both; the count is accepted and **not** honoured, a batch runs once. And
`BodyScan`: a `CREATE`/`ALTER`/`CREATE OR ALTER` `PROC`/`FUNCTION`/`TRIGGER` takes the rest of
its batch as its body, `;`s and all, so only a `GO` or the end ends it — SQLite's `TriggerScan`
problem with a different terminator. **And `BatchScope`: what lives in one batch is never cut at
a `;`.** Each piece Run Everything or a `.sql` run sends is a batch of its own, and a batch is
T-SQL's unit of scope — a variable lives in one, and a `BEGIN … END`, a `TRY … CATCH` and an
`IF … ELSE` are each one statement to the server — so cut at their `;`s, `DECLARE @id int;
SELECT @id = 5; SELECT @id` failed at its second piece (Msg 137) and a block's first piece was a
syntax error (Msg 102), after the pieces before it had committed (measured on 2022).
`scan_bounds` tracks it on `batch_separator` dialects alone. A `;` does not split inside a
`BEGIN`/`CASE` block, depth counted — `BEGIN TRAN[SACTION]`, `BEGIN DISTRIBUTED`, `DIALOG` and
`CONVERSATION` open none, and `END CONVERSATION` closes none — nor directly before an `ELSE`,
which `next_code_word` looks past the `;` for; its `Lookahead::Pending`, when a chunked scan's
buffer ends first, makes the splitter wait for the next block rather than cut. And from a
`DECLARE`, a `RETURN`, a `GOTO` or a label the rest of the `GO` batch is one piece
(`BatchScope::whole`). A label is any word directly before a single `:` — `::` is a scope
qualifier, a qualified name's part is none — **wherever it stands**: the scan once took one only
after a `;`, a `GO` or the start, and T-SQL needs no `;`, so `SELECT 1\nagain:\nPRINT 2;\nGOTO
again` sent `GOTO again` alone (Msg 133, measured on 2022) after the piece before it had run. A
`@x`, `@@x` or `#t` is stepped over whole
(`t_sql_name_prefix`, `continues_name`), so `@declare` declares nothing. **It is a middle
ground, on purpose**: sending whole `GO` batches, as sqlcmd does, is the simpler rule and would
have collapsed Run Everything's per-statement results into one panel per batch, so statements
that share nothing still go one by one. The editor's statement bounds and Run at the caret read
the same scan, so the caret inside a block runs the whole block
(`a_t_sql_batch_scope_is_never_cut_at_a_semicolon`, chunked as well; live,
`a_batch_scoped_script_runs_whole`). **Run at the caret stops at the caret's statement**,
though: it took the scoped piece whole, so Ctrl+Enter on a `SELECT` below a `DECLARE` ran the
`DELETE` after it too, and when the piece was the whole buffer nothing outlined it.
`run_current_range` (which `executable_at` sends) cuts the piece again at the first `;` past the
caret that only `BatchScope::whole` held — `scan_bounds_with`'s `soft` list, never inside a
block nor before an `ELSE` — so the scope's start through the caret's statement runs, and the
statements below it do not. The editor's statement for completion and the AI actions stays the
whole piece, where the variable is declared. Ctrl+Enter opens the menu with that range outlined
whenever it has neighbours — code outside it, by `sql::holds_code`, so a comment beside a lone
statement is none and Ctrl+Enter just runs it (`statement_has_neighbours`) —
**or holds several statements** (`editor_pane::run_menu_wanted`), so a
write above the caret is shown before it runs
(`run_at_the_caret_stops_at_the_carets_statement_in_a_batch_scope`,
`ctrl_enter_outlines_a_batch_scoped_piece_before_running_it`). **None of that applies to a
selection**: Ctrl+Enter over selected code runs exactly it, split into its statements
(`sql::selection_statements`, over `editor_executable_spans` — cut as Run Everything cuts, so
Select All over a script written without `;` runs it piece by piece rather than sending it as one
statement the server refuses, `a_selection_is_cut_at_its_blank_lines_as_run_everything_cuts`)
and handed to the guarded `run` —
or `run_all` for several — with no menu, a fragment of a statement included
(`a_selection_runs_as_its_statements`). **A multi-region selection** (an Alt+Shift box, several
carets) runs every region's statements in document order (`sql::regions_spans`), where floem's
single-cursor `get_selection` had run the first region alone while every line showed selected;
carets with nothing selected run nothing and say so on the guard bar, since no region names a
statement (`several_regions_run_every_region_in_document_order`). The missing-`WHERE` guards lose nothing by the larger
piece, since `tsql_statements` (below) cuts at statement words and not at `;` — and the `.sql`
panel's probe, which read a piece by its head and so missed everything after a `DECLARE`, now
counts through it too (`core::script`). **`nested_block_comments` was a divergence the table did not have**:
PostgreSQL nests `/* … */` and `skip_comment` ended every comment at its first `*/`, so a head
after a nested comment was read from inside it — `/* a /* b */ c */ DELETE FROM t` ran its
every-row DELETE with no ask — and a quote in the comment's tail opened a "string" that hid a
real second statement from every gate (`a_nested_block_comment_ends_where_its_dialect_ends_it`;
PG 16 was checked to read `SELECT /* a /* b */ c */ 42` as one comment). The highlighter's
continuation of a comment across lines still looks for the first `*/`, a colouring matter
only. The
scanner used to ask `dialect == Postgres` / `!= MySql`, which silently sorts any *third* engine
onto whichever side each comparison happens to put it — with nothing failing to compile, since
`!=` is exhaustive over any number of variants. Three of those defaults were wrong for SQLite
and two dangerously: it has no `\` escape inside a string, so `'C:\'` under MySQL's rule latches
the scanner into a literal that never ends and swallows the rest of the statement, which is
exactly how a `WHERE` gets hidden from the guard this module was consolidated to kill. Adding an
engine now fills in the table rather than hoping a `!=` falls the right way. `comment_open` is
the classification half, exposed because `pairs::region_at` has to tell a comment span from a
string span after the lexer has found one and was answering it with its own byte test.
`is_one_comment` asks whether a text lexes as exactly one comment closed at its own last byte —
the question an editor asks before writing `/* … */` round something. `skip_noncode` alone
cannot answer it, since an **unclosed** comment runs to the end of the input too: `/* a /* b */`
is one comment on MySQL and still open where comments nest, and came back the same length both
ways. So it lexes the text with `\nx` after it and asks for the text's own length
(`one_comment_is_closed_at_its_own_end`).
**`noncode_kind` → `NonCode::{Comment, Literal, Identifier}` is the whole of that
classification, and it is here for the same privacy wall.** A caller that has to *colour* or
label a span rather than skip it needs the same per-dialect predicates the scanner uses, and the
syntax highlighter was instead asking `skip_noncode` where a span ran — dialect-aware — and then
classifying it by its opening byte with no dialect at all, in a `match` whose arms were
`` ` ``, `'`/`"`, and everything else a comment. `skip_noncode` also answers `Some` for `$` on
PostgreSQL and `[` on SQLite, so both fell onto the comment arm: **every dollar-quoted function
body and `DO` block in a PostgreSQL editor was greyed out as though commented**, every
bracket-quoted identifier on SQLite was, and `"Name"` painted as a string on the two engines
where it is a name. The highlighter's own field doc promised the opposite — the `#` half worked
and the `$tag$` half was exactly inverted. `None` exactly where `skip_noncode` answers `None`,
which is what makes a `$` opening no valid tag ordinary punctuation on both sides.
**A `$` that continues a name opens nothing.** PostgreSQL lets an unquoted identifier carry `$`
after its first character, so `a$$` is one name and a dollar quote opens only where a token
starts. `scan_dollar` opened one after the `a`, so `SELECT 1 AS a$$; SELECT pg_sleep(100); SELECT
1 AS b$$` lexed as a single statement holding a string — while PostgreSQL 16.15 answered the same
shape (`SELECT 1 AS a$$; SELECT 2; SELECT 3 AS b$$`) with three result sets, columns `a$$` and
`b$$`. The headless read path sends through `simple_query_raw`, which takes several statements,
so the read-only gate's one-statement count and every word scan after it were bypassed, and
whatever sat between the two names ran. `scan_dollar` now asks about one byte, the one before
the `$`, through `continues_dollar_name` (`is_word_byte` or `$`), and answers `None` when it
continues a name. `noncode_kind` asks `scan_dollar` too, so the two still agree, and
`call_tokens` asks the same predicate where a name ends, so the function gate's tokens and the
lexer cannot disagree about it — the disagreement the smuggle lived in. **One byte rather than
the run is load-bearing**: the first spelling walked back over the whole name before every `$`,
so `a$a$a$…` rescanned everything before each one — quadratic, on text a model can send to
`run_query` and the editor splits on every keystroke (`pg_a_dollar_dense_name_lexes_in_one_pass`).
The byte rule also opens nothing after a digit or a closing dollar-quote tag (`1$$x$$`,
`$a$x$a$$b$`); a quote opening straight after either does not parse, so the difference can only
over-block. A keyword is a name to this rule like any other — `SELECT$$x$$` is the identifier
`select$$x$$` — and
`pg_a_dollar_inside_a_name_opens_no_quote` pins both halves: the smuggle is three statements to
the gate and the splitter, and a quote still opens after a space, a `(`, an `=` and a string.
**The unattended read-only gate's allowed heads are a per-dialect list too** — `read_only_heads`,
which `read_only_reason` both tests against and builds its rejection message from:
`SELECT/SHOW/DESCRIBE/DESC/EXPLAIN/WITH` on MySQL, `SELECT/SHOW/EXPLAIN/WITH` on PostgreSQL
(`SHOW search_path` is real SQL there, while `DESCRIBE` isn't — psql's `\d` is a client command
rather than a statement), `SELECT/EXPLAIN/WITH` on SQLite, and `SELECT/WITH` on SQL Server,
whose plan is `SET SHOWPLAN_XML ON` — session state, not a head. One shared list was wrong in both
directions at once for a third engine: it waved `SHOW TABLES` through to SQLite, which has no
such syntax, so the model got a raw parser error instead of being told the engine has no such
thing, and a rejection named heads that connection couldn't use — which a model will keep
retrying. It is also what the MCP server builds `run_query`'s advertised description from, so
the tool text and the gate can't drift
(`the_read_only_heads_are_the_ones_the_engine_actually_has`,
`the_rejection_lists_only_this_engines_heads`). The single-statement check applies the same
everywhere; the deny scan — which is what refuses a write hidden behind a `WITH` head — is
`DENY_ANY_ENGINE` for every engine plus `deny_keywords_for`'s per-dialect half, since each
engine's file, sleep and lock primitives share nothing but their effect — MySQL's and SQLite's,
that is: PostgreSQL's are function calls, and are answered by an allowlist instead (below).
SQL Server has both halves. Its deny list is T-SQL's statements (`EXEC`, `DECLARE`, `WAITFOR`,
`BACKUP`, `DBCC`, `BEGIN`, `INTO` for `SELECT … INTO`, …) and its table hints (`UPDLOCK`,
`HOLDLOCK`, …), which `locking_clause` names as locks rather than as writes; T-SQL needs no `;`
between statements, so any of them may follow a `SELECT` as a second one, and only the scan over
every word finds it there. `OPENROWSET`/`OPENQUERY`/`OPENDATASOURCE` are on it because they read
a server file or send text to another server this gate cannot read. `WHILE` and `GOTO` are on it
as T-SQL's `BENCHMARK`: a loop writes nothing for the rolled-back session to undo, and runs until
it is stopped — on the unattended paths, by nobody (`a_sql_server_loop_is_not_a_read`).
**`NEXT VALUE FOR` is a phrase, asked on every dialect** (`advances_a_sequence`): `NEXT` and `VALUE` are ordinary column
names, so it cannot be a denied word, and no rollback returns a sequence — and MariaDB 10.3+
takes the phrase too, so `SELECT NEXT VALUE FOR s` had been passing there as a read.
**The gate has two front ends now, so its wording names neither.** It was the AI's alone and its
refusals said so; `schemaic query` runs the same function, and a person typing a `SLEEP()` at a
prompt being told it *"is not permitted in an AI query"* is being answered about somebody else's
session. The denied-keyword refusal says *read-only query* instead. The head-list refusal above
never named a front end, which is why only the one sentence changed.
**It is a text gate, and it is not what makes a statement read-only.** A head is a spelling, and
the gate cannot see what a function does: `SELECT setval('s', 1000)`, `SELECT lo_unlink(…)`,
PostgreSQL's `SELECT 1 INTO newtable` and a `SELECT` of a user function whose body `DELETE`s all
open with a read head and name no denied word, and all of them wrote through MCP `run_query` and
`schemaic query` — shown live on PostgreSQL 16 and MariaDB 10.11 — while this document called the
gate read-only. What refuses them is the session (`Db::fetch_query_enforced` with
`Enforce::ReadOnly`, under `schemaic-db`) — and the editor's `run_verdict` reads the same kind of
text, so a read-only connection's editor runs take that session too (the write-guard invariant
lists the five paths); the gate stays in front for the one-statement count and
for what a read-only session still allows — sleeps, locks, server-side file reads, and on
PostgreSQL the writes that are not transactional (below) and a `set_config` that turns the
session's read-only default off for the statements after it. On a read-only connection
`run_verdict` asks it too, of each statement range rather than of the whole text, on every engine
(the write-guard invariant has why it once asked only on SQL Server). **And the
one-statement count is only as good as the server's agreement about where a statement ends.**
`skip_noncode` assumes `\` escapes inside every quote, which a MySQL/MariaDB server stops doing
under `NO_BACKSLASH_ESCAPES`, and under `ANSI_QUOTES` it reads `"a\"` as an identifier with no
escapes at all — so `SELECT 'a\'; DELETE FROM t; -- '` is one `SELECT` here and three statements
there (`a_backslash_quote_hides_a_second_statement_from_the_gate` pins the gate's half: it
passes). The gate cannot know the server's mode, so the session is put in the one it assumed.
`mysql_mode_lexed_like_the_gate` removes every name in `MYSQL_MODES_THAT_MOVE_A_QUOTE` and keeps
the rest — the combination modes `ANSI`, `DB2`, `MAXDB`, `MSSQL`, `ORACLE` and `POSTGRESQL` are on
the list because each implies `ANSI_QUOTES`, and dropping the flag while keeping the combination
would have the server put it straight back (`a_combination_mode_that_implies_ansi_quotes_goes_too`).
It removes names rather than overwriting the mode, for `export::MYSQL_LITERAL_MODE_SQL`'s reason:
the strictness the server was configured with is not its business, and what a combination implied
apart from the quote survives as the flags the server lists beside it.
`mysql_mode_is_lexed_like_the_gate` is the read-back check the session makes *after* the `SET`,
matching whole names so `NO_BACKSLASH_ESCAPES_X` is not a hazard
(`a_mode_name_is_matched_whole_not_by_substring`). **The client character set moves a string's
end the same way**, and `mysql_charset_is_lexed_like_the_gate` is the session's read-back of
it: it refuses the five multibyte sets whose second byte can be `0x5C` (`big5`, `cp932`, `gbk`,
`sjis`, `gb18030` — MySQL's own escaping documentation's list), where `丁\'` reads as two
characters and a closing quote, the wide sets that are no client set at all (`ucs2`, `utf16`,
`utf16le`, `utf32`), and an empty answer. Naming the hazards rather than the safe sets is
complete because the server's list of character sets is closed
(`a_client_charset_is_lexed_like_the_gate_unless_a_quote_hides_in_a_character`).
**A quoted name in front of a `(` is a function call, and the deny scan has to see it.**
PostgreSQL resolves a double-quoted identifier against the stored lower-case `pg_proc.proname`,
so `SELECT "pg_read_file"('/etc/passwd')` is the same call as the unquoted one — while
`word_tokens` skipped the whole run as non-code, the name never became a token, and `is_denied`
was never asked about it, which put every PostgreSQL entry one character from reachable,
`lo_export` among them. It fires only ahead of a `(`: a quoted *column* of the same spelling is a
read, and `SELECT "delete" FROM t` staying `Ok` is what this gate was tuned for. **What
separates the name from its `(` is asked through `sql::next_code_byte`, never
`is_ascii_whitespace`.** The first spelling of the rule tested the first non-whitespace byte, and
a comment is not whitespace, so `SELECT "pg_read_file"/*x*/('/etc/passwd')` put the name straight
back out of reach — on a predicate that, at the time, `app/mcp.rs`'s `run_query` was gated on
alone, and that `schemaic query` is gated on too now. Every dialect
steps over a comment between a name and its argument list exactly as it steps over a space;
`next_code_byte` is built on `skip_noncode` and `noncode_kind`, so a comment is a gap while a
string or a quoted identifier — code rather than a gap — stops the scan. The repair is inside
`word_tokens`, so `contains_write`'s read-only gate, which had the identical door, closed with
it.
**A write keyword followed by an argument list is the builtin of that name** (`is_builtin_call`,
GitHub issue #5). `REPLACE` is denied for MySQL's `REPLACE INTO`, and was matched wherever it
stood, so `SELECT REPLACE(name, 'a', 'b') FROM t` was refused as a write on every engine — by this
gate, and through `WRITE_KEYWORDS` by `contains_write` on a read-only connection's editor — and
MySQL's `INSERT(str, pos, len, new)` and `TRUNCATE(x, d)` with it. `called_word_tokens`, which
`word_tokens` now wraps, marks each word with whether it is an **unquoted** name a `(` follows,
the gap asked through `next_code_byte` as above; both scans skip a word that is called and is a
builtin on that dialect — `REPLACE` on every engine, `INSERT` and `TRUNCATE` on MySQL/MariaDB
alone, so `insert(a)` stays refused elsewhere. It rests on one fact of the grammar: no statement
those words begin is followed by a `(`, so `REPLACE INTO`, `INSERT OR REPLACE` and SQLite's
`WITH … REPLACE INTO`, which a read head begins, stay refused
(`a_write_keyword_called_as_a_function_is_the_builtin`,
`a_write_keyword_not_called_is_still_refused`). A quoted name before a `(` is marked *not*
called, on purpose: whether a quoted spelling reaches the builtin or a user function differs by
engine, the mark exists only to let a builtin through, and refusing what nobody needs to spell
that way is the safe direction. What the text cannot tell from the builtin is an owner's
function of the same name, and that is `Enforce::ReadOnly`'s to refuse, as for any function that
writes. PostgreSQL's and SQL Server's call allowlists already listed `replace`, so they needed
no change.
**A read-only session does not refuse a write that is not transactional, so on PostgreSQL the
text gate has to judge functions.** Replication slots and origins, statistics resets, WAL
switches, restore points, backup markers, `pg_promote` and WAL-replay pause never call
`PreventCommandIfReadOnly`, and live on PostgreSQL 16.15, under exactly the `SET SESSION
CHARACTERISTICS AS TRANSACTION READ ONLY` session the headless read path runs on,
`pg_drop_replication_slot`, `pg_create_*_replication_slot` and `pg_stat_reset` all ran. A dropped
slot takes a CDC consumer's position with it; a created one pins WAL until the disk fills
(`the_gate_refuses_postgres_admin_writes_a_read_only_session_allows`, which also pins that the
`_peek_` forms of `pg_logical_slot_*_changes` leave the slot where it was and stay reads, while
the `_get_` forms consume it). **A function that takes SQL text is an eval**, and a string literal
is skipped whole, so every refused name was one quoted argument from reachable:
`query_to_xml('select pg_read_file(''PG_VERSION'')', …)` returned the file on the same server
while the bare `pg_read_file` was refused. The `query_to_xml*` functions, `ts_stat`,
`ts_rewrite`, the `dblink` family — whose own connection is not the read-only session, so it
writes as well — and `tablefunc`'s `crosstab*` and `connectby` are that door
(`a_function_that_evaluates_sql_text_is_refused`). `table_to_xml` and its siblings take a
relation rather than SQL, and the deny list left them as reads; they are not on the allowlist
below either, so they are refused now — a refusal, not a finding that they act.
**All of that used to be a deny list in `deny_keywords_for(SqlDialect::Postgres)`, and an
enumeration of writers fails open**: a new release or an extension adds a function of either kind
and nothing notices, and no list can name a function the database's own owner wrote. So that arm
now names **no function at all** — only `COPY`, which on PostgreSQL reads and writes server-side
files, and `UESCAPE`, which puts a string literal between a quoted name and its `(` and rewrites
the name the allowlist would have compared (`a_unicode_escaped_name_is_refused_on_postgres`) —
and every PostgreSQL call is asked of an **allowlist**, `PG_READ_FUNCTIONS`: builtins every
overload of which only reads (no file, sleep, lock, signal to another backend, session state,
sequence, large object, evaluated SQL text or write the session cannot see). A name not on it is
refused, and that is every extension function and every function the owner defined *under a name
of its own* — the gate cannot read a body, and a function's `STABLE` is its author's claim; an
owner's overload of a listed name is another matter (below). `read_call_policy` picks the list by an exhaustive match — `Some` for
PostgreSQL and SQL Server, `None` for MySQL and SQLite, whose builtin sets are small and closed
and keep their deny lists. SQL Server's builtins are closed too, but a user-defined function may
call an extended stored procedure and a CLR one does whatever its assembly may, so a name the
gate cannot vouch for is refused there as well. What differs between the two is a `CallPolicy`
rather than a second scanner: the lists (`MSSQL_READ_FUNCTIONS` unqualified, and
`MSSQL_SYS_READ_FUNCTIONS` only under `sys.`, since T-SQL builtins take no schema and `sys`
functions need one — so an owner's `dbo.fn_my_permissions` is not vouched for by its namesake),
the catalogue schema, the paren keywords (`MSSQL_PAREN_KEYWORDS`, the reserved words T-SQL puts
before a `(` that is not a call's, which can never name a function — `INDEX` and `TABLESAMPLE`
among them, without which the table hint `WITH (INDEX(ix))` and a `TABLESAMPLE (10 PERCENT)`,
whose `(` follows the keyword rather than a method name as on PostgreSQL, were refused as calls
to unlisted functions; `a_sql_server_hint_or_sample_clause_is_not_a_call`), whether `::` is a
cast (on SQL Server
`Type::Method(…)` calls a CLR static method, so it is a call), and whether a quoted name can
reach a builtin (on SQL Server it resolves as a user object). SQL Server publishes no catalogue
of its builtins, so the list was checked on 2022 (16.0.4295) by calling each name: error 195 is
"not a built-in", and `mssql::every_allowlisted_function_is_a_builtin` in the live tier holds it
there. The refusal's `writes` is `false`, since the fix is not `exec`
(`a_postgres_function_off_the_read_list_is_refused`). In `read_only_refusal` it comes **after**
the denied words, so a deleting CTE is still named as its `DELETE`, and **before** the locks,
whose "drop the clause" retry it would otherwise refuse a second time.
**Finding the calls is `call_tokens` and `unlisted_call`.** `call_tokens` is a token stream on
`skip_noncode`/`noncode_kind` boundaries which, unlike `word_tokens`, keeps punctuation, drops
comments, and keeps a quoted identifier's name case-sensitive, as PostgreSQL does — `"LOWER"` is
not `lower` — so the quoted-name and comment doors above stay shut for it by construction. A call
is a name before a `(`, quoted or not; a **qualified** call is listed only under `pg_catalog`,
since `public.lower` is whatever the owner defined under that name
(`a_schema_qualified_call_is_listed_only_in_pg_catalog`); and — PostgreSQL's own — a name after a
closing `)` or `]` and a `.`, because field selection on a value that is not a row *is* a call,
with no parentheses at all: on PG 16.15 `SELECT ('1'::float8).pg_sleep` slept, and
`(ARRAY[…])[1].f`, `(x) . "f"` and `(x)./*c*/f` all ran
(`a_field_selection_on_a_parenthesised_value_is_a_call`). `alias.f` and `alias.col.f` were
measured *not* to reach a function taking text ("column does not exist", "missing FROM-clause
entry"), so a dot after a name is left alone. **It is a token scan rather than `core::intel`'s
AST, against the rule that structure goes through the parser, and deliberately**: a refusal gate
has to fail closed, and here anything *spelled* as a call is asked about unless a named grammar
position says otherwise, so a construct nobody anticipated is refused. An AST visitor fails the
other way — it collects calls from the node shapes it knows (`Expr::Function`, a table function,
`UNNEST`), and a shape it misses is a call nobody asked about, which is the enumeration the
allowlist replaced. And the parser is not PostgreSQL's: a statement it rejects would be refused
outright, and one it reads differently from the server is the disagreement every text gate here
has been bypassed through.
**Where a name ends is `sql::continues_name`, and on SQL Server that is past a `#`, `@` or `$`.**
`dbo.h#`, `purge@now` and `v$as` are each one regular identifier there (measured on 2022), and
the scan used to stop at the `#`/`@`: `SELECT dbo.h#()` was then no call at all and
`SELECT dbo.f@GETDATE()` a call to the allowlisted `GETDATE`, so an owner's function ran on
`schemaic query` and `run_query`, and what it did outside the database (`xp_logevent`) survived
the rolled-back transaction (review finding R1-L2-01). `call_tokens` continues a word with
`continues_dollar_name || continues_name`, past `$` on every engine and past `#`/`@` on SQL
Server alone; a *leading* `@`/`#` is still a `Punct`, so `@@ROWCOUNT` and `#t` read as before
(`a_t_sql_name_holding_hash_at_or_dollar_is_one_call`).
**What is *not* a call is where the list's usability lives, and every rule is a name the
allowlist never sees**, so each is written to be sound rather than generous. `PG_PAREN_KEYWORDS`
holds only PostgreSQL's *reserved* and *column-name* keywords that the grammar puts before a `(`
— `IN`, `EXISTS`, `AS`, `GROUP`, `VALUES` — because those are the words it never accepts as an
unquoted function name. The first spelling also held `EXPLAIN`, `JOIN`, `ILIKE` and `SIMILAR`,
and PostgreSQL takes all four as function names (`EXPLAIN` is unreserved, the other three are
`type_func_name` keywords), so an owner's `join(1)` passed untouched
(`a_keyword_that_can_name_a_function_is_asked_about_as_a_call`).
`no_word_skipped_before_a_paren_names_an_unlisted_builtin` holds the builtin half of that line —
it fails on a skipped word that is also a builtin off the list, and caught `like` and
`overlaps`, which moved to the allowlist; the keyword categories are PostgreSQL's appendix C, and
no test holds that half. `paren_is_grammar` adds the positional rules (renamed from
`pg_paren_is_grammar` when SQL Server began reading through them with its own
`MSSQL_PAREN_KEYWORDS`, and `::` there a method call rather than a cast): any name after `AS`,
after `::` (a type's modifier), after `TABLESAMPLE`, or straight after a **value** — a `)`, a
`]`, a literal or number, a quoted name. The grammar never juxtaposes a value and a call, so there
it is an alias's column list (`(VALUES (1)) v(n)`, `FROM "t" x(a, b)`) or `OVER`/`FILTER` —
**except `OPERATOR`**, which stands exactly there and reaches an operator in any schema, so
`OPERATOR(…)` is refused wherever it stands; the rule when it knew only `)` had waved it through
after a parenthesis while refusing it after a literal
(`a_qualified_operator_is_refused_wherever_it_stands`). Then the words that are keywords only
where they stand: `BY` after `ORDER`/`GROUP`/`PARTITION`, `SETS` after `GROUPING`, `VARYING`
after `CHARACTER`/`CHAR`/`BIT`, `FIRST`/`NEXT` after `FETCH`, `MATERIALIZED` after `NOT`,
`EXPLAIN` only at the head, and `JOIN` only after a join-type word
(`INNER`/`OUTER`/`LEFT`/`RIGHT`/`FULL`/`CROSS`/`NATURAL`) or before a relation —
`opens_a_relation`, which asks whether the group opens with `SELECT`/`VALUES`/`WITH`/`TABLE`,
as no argument list can, or holds a top-level `JOIN` of its own, as a parenthesised join does.
Asking the word alone would let a function of that name through everywhere else.
`cte_column_list_names` marks a CTE declared with a column list only
when the whole `WITH [RECURSIVE] name(…) AS [NOT] [MATERIALIZED] (` header matches, and marks
that *position*, so a CTE named after an unlisted function does not open the function elsewhere
in the statement (`a_declared_name_is_not_a_call_but_does_not_open_the_function`).
`ordinary_postgres_reads_pass_the_function_allowlist` is the other side of it — the reads a model
writes every day, which these rules exist to keep passing.
**Two tests hold the lists themselves.** `every_listed_function_is_a_postgres_builtin` checks
each name against `pg_builtins::PG_FUNCTIONS` plus a documented grammar-only set (`rollup`,
`cube`, a type written with its modifier, a `TABLESAMPLE` method, and `xmltable`,
`xmlattributes` and `xmlnamespaces`, which have no `pg_proc` row), since a misspelt entry is a
function nobody can call; `every_function_the_deny_list_named_is_still_refused` asserts that every
retired entry is refused now, bare and quoted, because that list was the record of what had been
found to act. A name the allowlist lacks costs a refusal, never a write.
**The known over-blocks are all in the safe direction**: a table alias with a column list but no
`AS` after an unquoted table name (`FROM t x(a, b)`; after a quoted one it passes), and a
composite field read through parentheses (`(x).key`). `replace(…)` was a third — listed on the
allowlist and refused anyway by `DENY_ANY_ENGINE`'s `REPLACE` — until a called builtin stopped
being a denied word (`is_builtin_call`, above). **What the
text cannot see rests on `Enforce::ReadOnly`**: a view's body, a trigger, an operator's function,
the whole-row `alias.f` form, which reaches only a function taking a row or `any` — in
`pg_catalog` all formatters (`row_to_json`, `to_json`, `pg_typeof`, measured through `pg_proc`)
— and **the gap between a name and a function**. The list holds names, and PostgreSQL picks the
overload whose argument types match best across the whole search path: on PG 16.15, with an
owner-defined `public.lower(integer)` in place inside a rolled-back transaction, `SELECT
lower(1)` returned the owner function's value though `lower` is listed. An owner's implicit cast
likewise calls its function with no name in the text at all. That session does not refuse the
non-transactional writers above, so one of them reached any of these ways — behind a view, say —
is out of both halves' sight; that follows from the two measured facts rather than being
measured itself. Of the writes this entry lists as passing the gate, the three that are calls —
`setval`, `lo_unlink`, an owner's function under a name of its own — are refused by it on
PostgreSQL now; `SELECT 1 INTO newtable` still passes, as does a function on
MySQL/MariaDB, and the session is what refuses those. Reach is why the direction matters —
`schemaic mcp` puts this gate in front of any MCP client, behind tools that all advertise
`readOnlyHint: true`.
**`/*! … */` is two questions about the same bytes, and both answers now exist.**
`SqlDialect::executable_comment` is the capability — MySQL and MariaDB *run* what is inside one,
and a bare `/*!`, a versioned `/*!50000` and MariaDB's `/*M!` are all treated as code, a lexer
that guessed the server's version guessing in the unsafe direction half the time. `skip_comment`
therefore stops just past the **marker**, because its callers are the security gates and they
must see the body as the code the server will run. `executable_comment_end` is the other answer:
the index past the whole run, marker, body and closing `*/`, for a caller asking what it may
re-flow rather than what will execute. See `sqlfmt.rs` for what taking the gates' answer cost.
**The gate's *lexer* half is per dialect as well, and is where the real bypass was.** Whether a
statement has ended is a dialect question, so the same text gets different — and individually
correct — verdicts: `SELECT 'a\' ; DELETE FROM s; --'` is one statement on MySQL, whose
backslash escape swallows the rest of the literal, and two anywhere else. A SQLite connection
gated with `SqlDialect::MySql` therefore passed a payload that deletes a table (`c6c5dae`).
The tests read `EVERY_DIALECT` rather than the module's MySQL-binding helper, and three
(`the_gate_reads_this_engines_string_escape` / `…_comment_rule` / `…_identifier_quoting`) assert
that the engines' answers *differ*, so a mis-paired dialect fails instead of passing by
coincidence. A dollar quote opened inside a name was the same kind of bypass on PostgreSQL alone
— `a$$ … b$$` lexed as one string what the server ran as three statements — and is the lexer's
to answer (above).
**`scan_bounds` is the one walk both `statement_bounds` and `statement_bounds_open` are, and its
`until` is what keeps the caret off it.** With `Some(offset)` the walk stops once a boundary
*past* that offset has been recorded — everything after belongs to statements the caller has
already said it does not care about — and the boundaries produced before the stop are
byte-for-byte the ones the full walk produces, this being the same lexer cut short and not a
second one. It exists for `statement_range`, which answered "which statement is the caret in" on
**every caret move**, undebounced, through `update_signature_help`, and which nothing caps:
`sqlfile::open_verdict` will open a 64 MiB script, and over the whole document the walk measured
5.0 / 20.5 / 80.7 ms at 1 / 4 / 16 MiB — an arrow key's worth of UI-thread work to locate a
statement whose actual work, `intel::signature_help` on the isolated statement, is 8 µs. `None`
keeps the full walk, which is what every other caller wants. **Cut short was not short enough,
and the per-move callers now ask a `StatementIndex` instead** (below): with the caret mid-file
the walk to it still measured 6.3 / 25 / 104 ms at 1 / 4 / 16 MiB (release, a realistic
script), and the run outline paid it a second time on the same move. The inline prediction was
the last per-keystroke caller left on the walk: its gate ran it twice and `Recall::at` once more,
so a space typed at a line's end in a 4 MB MySQL script cost about 100 ms (33 ms a walk,
release) before any producer ran; through the index the three measure under 4 µs together
(`predict_indexed`, under `intel.rs`). `until` stays for the walks that remain — Ctrl+Enter,
the menu actions — one call per press.
**`StatementIndex` is one revision of a buffer's boundaries, and what a caret move asks.**
`new` is one full walk (`scan_bounds_with`, soft `;`s collected), keeping the walk's bounds
closed by the document's end, where a `DELIMITER` directive changes the terminator in force
(`delims`), the soft `;`s, and — the first time a caret is in the unterminated tail — that
tail's blank-line cuts (`tail_cuts`). Its `statement_range_ended`, `statement_range`,
`writing_range` and `run_current_range` answer exactly what the free functions do, by binary
search: the walk's bounds up to the first past the caret, or closed by the end, which is the
list the cut-short walk produces. That is why the pick is shared rather than copied —
`statement_at` and `run_current_at` take the boundaries as slices from either source — and why
`caret_segment` searches (`partition_point`) rather than looping, handed a whole document's.
`after_edit(new, pre, suf)` takes the index forward over an edit already known — the front and
back a `text::EditLog` hands over, so no copy of the old text is kept; `text_len` is the length
it was built for, which its holder checks before trusting one — and `after(old, new)` is the
same after finding the edit with `text::common_affixes`. It resumes **two** boundaries before it — the last one's own decision may
have read into the edit, as T-SQL's `IF … ; ELSE` lookahead does — and scans a statement at a
time through `scan_bounds_from`, stopping at the first boundary past the edit that the old scan
also recorded at the shifted position, with the same `strip` and the same terminator in force;
the old boundaries and soft `;`s after it, shifted, stand for the rest. **It resumes at a
recorded boundary of the whole text, never at a slice**, which would put a mid-line `GO` at a
line start; resuming is exact because every state the scan carries resets at a boundary but the
terminator. Measured (release, a realistic script, after a one-character edit): `after`
0.09–0.16 / 0.49–0.58 / 3.5–6.6 ms at 1 / 4 / 16 MiB, a caret-move lookup ~0.0001 ms at any
size, and `new` 14 / 58 / 230–350 ms, once, on the first ask. The index's test found a scanner
bug on the way: on a `DELIMITER` directive the new terminator was set only *after* the `until`
stop, so a scan stopped at a directive handed back the old one. It is set before the stop now —
invisible to every earlier caller, none of which reads the terminator after a stop, and
load-bearing for `after`, which resumes with it. `a_statement_index_answers_what_the_walks_do`
edits per-engine scripts (MySQL `DELIMITER` and triggers, T-SQL `DECLARE`/`IF … ELSE`/`GO`/
`BEGIN … END`, a SQLite trigger, PostgreSQL dollar quotes, unterminated tails with blank lines)
at every byte — inserting `;`, newlines, `GO`, block words, quotes, comments, `DELIMITER $$` or
`$$`, or deleting — and holds each taken-forward index equal to a fresh one and its answers to
the walks', and takes every edit forward a second time through `after_edit` at half the shared
front and back, as merged edits hand them over. Who holds the index is under `completion.rs`.
**Which segment the caret means is `caret_segment`'s, the one choice `statement_range` and
`run_current_bounds` share** — so Run at the caret's outline and what `executable_at` sends
are chosen there too. It is the segment the caret falls in, save in two places where it means
the statement before: a blank segment, as after the final `;`; and the rest of the line a `;`
ends — only spaces, tabs or a `\r` between that bound and the caret, and nothing but whitespace
or a comment after the caret on its line. A segment starts straight after its `;`, so with
`SELECT 1;⏎SELECT 2;` and the caret at the end of the first line — the end of the line just
typed, where Ctrl+Enter is pressed most — Run Current ran `SELECT 2`, the statement below it.
Code after the caret on that line still means the next statement (`SELECT 1;| SELECT 2;`),
since the caret's own statement then starts there
(`a_caret_at_a_lines_closing_semicolon_is_in_that_statement`, over every dialect and through
`statement_range`, `run_current_range` and `executable_at` alike). **What the statement is and
whether the caret is in it are two questions**: `statement_range_ended` is the range plus
whether the choice fell back past that statement's terminator, and `statement_range` is its
first half. Falling back is right for Ctrl+Enter and wrong for anything that writes at the
caret, which is why `intel::predict` and `Recall::at` ask for the second half and refuse there
(`the_caret_is_past_a_statement_only_after_its_terminator`; see `intel.rs`). Completion asks
`writing_range`, the same pair as an empty range at the caret when it has fallen back, so
Ctrl+Space below `DELETE FROM t;` offers what starts a statement rather than the `DELETE`'s
`WHERE` (`the_writing_range_is_empty_past_a_terminator`).
**Where no terminator ends the statement, a blank line may — for Run at the caret alone.** Two
queries written one under the other without a `;` ran together on Ctrl+Enter, and the server
refused the pair. So when the caret's segment is the last one — the tail after the final `;`,
or the whole buffer when there is none — `run_current_bounds` asks `blank_line_cuts` where a
blank line ends a statement inside it, and narrows the piece to the cuts either side of the
caret. A cut is the first byte of the next statement, its leading comments included, so a caret
on the blank lines themselves is still in the statement above, as it is past a `;`. **It is a
fallback, never a second way to separate statements**, so the cut is refused wherever the
statement is plainly not over: inside parens, a string or a comment; inside a routine's body (a
`CREATE`/`ALTER` whose first eight words name a `PROC`, `PROCEDURE`, `FUNCTION`, `TRIGGER` or
`EVENT`, which then runs to the end of the tail), a T-SQL batch scope or block (`BatchScope`,
`BodyScan`) or a trigger's `BEGIN … END` (`TriggerScan`); unless the next code word opens a
statement (`STATEMENT_STARTS`); and where the code before it carries on (`blank_line_ends`) — a
trailing `,`, `(` or operator, a `CARRIES_ON` word such as `UNION`, `AS`, `FROM` or `AND`, or an
`INSERT`/`REPLACE` header whose `SELECT`, `WITH`, `VALUES` or `TABLE` comes after the blank
line. **Nor is a statement cut from the head that governs it** (`awaits_statement`): a T-SQL
`IF`/`WHILE`/`ELSE` with no statement after it yet, an `EXPLAIN`/`DESC`/`DESCRIBE` with only
its options, a `WITH` whose CTEs await their main statement. The first spelling had none of
these, so Ctrl+Enter on the `DELETE` under `IF EXISTS (…)` ⏎⏎ ran it unconditionally, and
under `EXPLAIN` ⏎⏎ ran the `DELETE` a plan was asked of. And a `WITH` that opens no CTE
(`opens_cte`) opens no statement: it is an options clause continuing the one above —
`CREATE LOGIN app` ⏎⏎ `WITH PASSWORD = …` was cut into a piece of its own that no credential
rule recognised, and its password went to `history.json`. Each kept-together shape was run
live as the one statement it is (SQL Server 2022, PostgreSQL 16, MariaDB 10.11). A statement a
`;` ends keeps its blank lines
(`a_blank_line_separates_statements_where_no_terminator_does`,
`a_blank_line_inside_a_statement_does_not_cut_it`,
`a_blank_line_does_not_cut_a_governed_statement_from_its_head`). **Run Everything cuts the same way**:
`editor_executable_spans` is `executable_spans` with the unterminated tail cut by the same
`blank_line_cuts`, read by the run menu's Run Everything, by a selection's Ctrl+Enter
(`selection_spans`), by the command palette's *Run* (`run_everything_statements`, those spans
sliced — it split with `statement_ranges` and so sent `END$$` and a `;`-less script as one
statement) and by the toolbar's Run — which sends
the pieces through `run_all` when a blank line cut something, and the whole buffer through `run`
as before when none did; all stay behind the write guard
(`run_everything_cuts_the_unterminated_tail_at_blank_lines`,
`run_everything_statements_drop_the_delimiter_and_cut_blank_lines`). **The rest is deliberately
terminator-only**: `statement_range` (completion, signature help, the AI actions),
`statement_ranges`, `executable_statements`, the script runner, a dump and `schemaic exec` — a
file is not a buffer someone is writing in, and a blank line inside a long statement there must
not cut it.
**`unsafe_reason` asks first about what destroys stored rows, and about nothing else.** It is
the warning behind `first_unsafe`, which `run_verdict` turns into `Confirm` whatever
`confirm_writes` says: `every_row_reason` (below), or `drop_reason`, a statement that destroys
stored rows *with the object holding them*. `DROP` was the gap: a CLI test run found
`schemaic exec "DROP TABLE t"` running with no `--yes` and exiting 0 with "(0 rows affected)"
while the less destructive `TRUNCATE` was held, and since this is the one guard both front ends
use, the editor's Run let it through unasked as well. **The first fix asked about `DROP
TABLE`/`DATABASE`/`SCHEMA` and called everything else rowless, which several spellings made
false** — MySQL's `DROP TABLES` among them, running unasked beside a held singular. `drop_reason`
now reads `DROP TABLE`/`TABLES`/`DATABASE`/`SCHEMA`; PostgreSQL's `DROP OWNED` (everything a role
owns); `DROP TYPE`/`DOMAIN`/`EXTENSION … CASCADE`, which takes every dependent column with its
values — without `CASCADE` the server refuses while anything depends, so there is nothing to
ask; MariaDB's `CREATE OR REPLACE TABLE`/`DATABASE`/`SCHEMA`, which drops what is there first;
and MySQL's `ALTER TABLE … TRUNCATE PARTITION`/`DROP PARTITION`, looked for through
`leading_words` within `PARTITION_WORDS` (10) words so the look stays at the statement's head.
The scope still stops on purpose. `DROP TEMPORARY TABLE` and `CREATE OR REPLACE TEMPORARY
TABLE` die with the session; a dropped view, index, trigger, routine or user holds no stored
rows, nor does a `CREATE OR REPLACE VIEW`/`FUNCTION`/`TRIGGER`, which is how every view and
routine is written there; and `ALTER … DROP COLUMN`, which does destroy values, stays out
deliberately as the everyday schema edit — a guard that fires on every DDL statement is one
people learn to click through (`dropping_a_table_database_or_schema_asks_first`,
`a_statement_that_destroys_stored_rows_by_another_name_asks_first`,
`dropping_what_holds_no_stored_rows_does_not_ask`, all over `EVERY_DIALECT`). The schema tree's
own Drop is not asked twice: it goes through the DDL preview and `Db::run_ddl`, never
`run_verdict`.
**The every-row half is `every_row_reason`, and `unsafe_reason` is it plus `drop_reason`.**
`script::probe` asks the half on its own for `Probe::unqualified`, the `.sql` panel's "act on
every row … with no WHERE" line: borrowing the whole guard put a dump's every `DROP TABLE IF
EXISTS` under that line, beside the destruction line that already counts it
(`a_dumps_drop_before_create_is_destructive_and_not_unqualified`, watched fail at 2). Beside the
missing-`WHERE` `DELETE`/`UPDATE` and the `TRUNCATE` it asks about MySQL's `ALTER TABLE …
TRUNCATE PARTITION ALL`, a `TRUNCATE` by another name.
**What the guard says is a value, not a sentence** — phase 4 of *Interface translation*.
`RunVerdict::Block`/`Confirm` carry a `GuardReason` — `ReadOnly`, `ReadOnlyRefused` with
`read_only_refusal`'s `ReadOnlyReason`, `NoDatabase`, `Unsafe`, `Writes { several }`,
`Script { file }` (`script_verdict`'s confirmation) and `Staged { after, changes }` (`filter::with_staged_question`)
— and `unsafe_reason`, `every_row_reason`, `drop_reason` and `first_unsafe` answer
`Option<Unsafe>`: what the statement destroys, with the SQL keyword it spelt as a
`&'static str` where there is a choice (`NoWhere { kind }`, `DropContainer { obj }`,
`DropCascade { obj }`, `ReplaceContainer { obj }`). Each has `message()`, byte for byte the
`String` it replaced, and no `Display`; the CLI says it, the editor words the value
(`ui::i18n::guard_reason_text`, under *The string catalogue*). `NO_DATABASE_SELECTED` stays as
`NoDatabase`'s English. The read-only gate's own refusal is a value too: `read_only_refusal`
answers a `ReadRefusal` whose `reason` is a `ReadOnlyReason` — `Several`, `NotARead { heads }`,
`Denied { word }`, `AdvancesSequence`, `UnlistedCall { name }`, `Locks { clause }`, each field
SQL the statement spelt — and `ReadOnlyReason::message` is the English, byte for byte the
sentences it used to return. `read_only_reason` still answers `Result<(), String>` with that
English, but no production caller reads the text any more: `filter.rs`, `write_log.rs` and the
MCP server (`cli/mcp.rs`) ask it only `is_ok()`/`is_err()`, as the live legs do, and only
`sql.rs`'s own tests hold its `Err` as text. `db::mssql` refuses through
`read_only_refusal` and carries the typed `DbRefusal::ReadOnly(reason)`; `cli::query::gate`
says `message()`, the CLI and the MCP server staying English. No decision moved with the type: every arm, its order and the statements it fires on
are as described above.
**Both halves judge the statement an executing prefix runs, not the prefix.** `EXPLAIN ANALYZE`
(with `VERBOSE`/`FORMAT=…`), PostgreSQL's `EXPLAIN (… ANALYZE …)` option list and MariaDB's bare
`ANALYZE [FORMAT=JSON]` execute what they explain, and every arm reads a head, so `EXPLAIN
ANALYZE DELETE FROM t` deleted every row with no missing-`WHERE` ask — through `schemaic exec`
with no `--yes` as well. `analyzed_statement` strips the prefix before either half reads it. A
plain `EXPLAIN` only plans and `ANALYZE TABLE t`/PostgreSQL's `ANALYZE t` gather statistics, so
neither is asked; an option list's `ANALYZE false` still counts as analysing, over-asking being
the safe direction (`an_analyzed_statement_is_judged_as_the_statement_it_runs`).
**On SQL Server both halves judge every statement in a range, not the range's head.** T-SQL
needs no `;` between statements, so one range `statement_ranges` cut at `;` and `GO` can hold
several, and every arm here reads a head: `DELETE FROM a WHERE id = 1\nDELETE FROM b` borrowed the
first statement's `WHERE`, and the second emptied `b` with no ask. `tsql_statements` cuts a SQL
Server range again before each top-level word that begins a statement, and `every_row_reason`
and `drop_reason` ask each piece — so `unsafe_reason`, `first_unsafe` and the `.sql` panel's
probes all do. It is **not a parser**, and does not have to be: it finds where the statements the
guards ask about *begin*, and a cut in the wrong place produces a fragment no guard answers for.
What it must not do is cut where the word continues the statement before it — a set operator's
second `SELECT`, `MERGE`'s `THEN UPDATE`/`DELETE`/`INSERT`, a cursor's `FOR UPDATE`, a foreign
key's `ON DELETE CASCADE`, `INNER MERGE JOIN`, `DROP … IF EXISTS`, an `ALTER TABLE`'s
`TRUNCATE PARTITION` and an `ALTER`'s `DROP` clause. That last is known by what follows it
*not* being a class of object (`tsql_drop_names_a_class`), inside a statement headed `ALTER`,
because T-SQL's `CONSTRAINT` is optional: `ALTER TABLE t DROP pk_t` runs (measured on 2022) and
was cut into a `DROP pk_t` the panel counted as a destruction, and `ALTER ROLE r DROP MEMBER u`
with it; while `DROP PARTITION`, once listed as a clause, hid a whole `DROP PARTITION
SCHEME`/`FUNCTION` after an unterminated statement
(`a_drop_after_an_alter_is_a_clause_unless_it_names_an_object_class`) — or anywhere after the head of a procedure, function, trigger or
view, which is one statement whatever it holds, or of a `GRANT`/`REVOKE`/`DENY`, whose privilege
list is made of those words. A leading `WITH` keeps the statement its CTEs feed, and text headed
`EXPLAIN`/`ANALYZE` — no T-SQL, but the prefixes `analyzed_statement` strips — stays whole. Every
other dialect gets its range back whole
(`every_sql_server_statement_in_a_range_is_judged_for_a_missing_where`, whose last assertion is
that MySQL still reads the first shape as one statement). `holds_several_statements` is the
same cut counted, for `db::mssql::run_statement`'s question of whether a result's end is the
piece's end; it errs toward *several*, but only over the statements the **text** holds, and a
procedure call is one statement there and any number to the server. So the row cap asks
`drains_past_row_cap` — several, or `contains_write` (which counts `EXEC` and every head outside
the read set) — and only a lone read keeps its early stop. A cut in the wrong place then drains
a result that could have been left: time, never an outcome.
**`limited_select` is the one spelling of a generated row cap**, because T-SQL has no `LIMIT`
and writes `TOP (n)` after `SELECT`: the generators had each written a trailing `LIMIT` into a
`format!`, which is an engine assumption no census can find. `filter::table_query` and
`mssql::fetch_table` build through it. The rest of SQL Server's arms here are the ones
already written for the others: `use_target` reads T-SQL's `USE` (bracketed names included),
`needs_database` has an arm because a login lands in its default database, usually `master` —
everything but a read, `USE`, session and transaction control, flow control (`IF`, `WHILE`,
`RETURN`, `THROW`, `RAISERROR`), the statements about the server rather than a database in it
(`KILL`, `BACKUP`, `RESTORE`, `DBCC`, `RECONFIGURE`, `SHUTDOWN`) and the server-level objects
(databases, logins, endpoints, …) needs one: `EXEC`, since a procedure can create anything, and a
`SELECT … INTO` or a `WITH` whose CTEs feed an `INSERT`/`UPDATE`/`DELETE`/`MERGE`, which are no
reads. The flow-control and server-level heads had been hard-blocked with *No database
selected.* **`mssql_needs_database` asks every statement in the range, in order**, through
`tsql_statements` (above): `SET NOCOUNT ON` on the line above a `CREATE TABLE` is one range, and
its head alone sent the table to `master` on an unscoped tab. The statement an `IF` guards is
asked on its own, so `IF OBJECT_ID('t') IS NULL CREATE TABLE t` still needs one, and a `USE`
answers for everything after it
(`a_sql_server_statement_behind_a_harmless_one_still_needs_a_database`). `DATABASE` is
server-level only as a database's own name: `DATABASE SCOPED …`, `DATABASE CURRENT`,
`DATABASE AUDIT SPECIFICATION` and `DATABASE ENCRYPTION KEY` name the current database or
something in it, so they need one too — unscoped, they landed in `master`. And
`no_database_failure` matches error 208,
*Invalid object name*, as the hint that the statement ran somewhere other than meant.
