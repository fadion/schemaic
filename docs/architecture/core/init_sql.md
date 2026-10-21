# `core/init_sql.rs`

`init_sql.rs` — **a connection's init SQL**: the session settings `Connection::init_sql` runs on
every connection Schemaic opens for it — a `search_path`, a time zone, a role — and `check(sql,
dialect)`, the gate they pass first. **It is user SQL that runs with nobody confirming it, on
every operation** — one connection per operation makes that a run, a listing, a ping, a `KILL` —
so the gate is an **allowlist** with no `Confirm` arm, the fourth of the refusals strictly
stronger than `run_verdict` that the write-guard invariant names. `check` answers the
statements, each trimmed and without its `;`, or the first refusal as a value: `InitRefusal`,
the refused statement's `history::preview` and an `InitWhy` — every reason, a named one
carrying the setting or scope lower-cased. The connection form words it in the interface's
language; its `Display` is the English the connect's error and the CLI still say, byte for byte
the `String` `check` used to return (`the_refusal_names_the_statement`). Empty SQL and a
comment alone are no statements (`nothing_is_nothing_to_run`). A
statement passes only as a `SET` — on SQLite, a `PRAGMA` — with no `(` at code level, since
`SET @x = f()` on MySQL or SQL Server calls a function and a function can write, and then **only
if the setting it names is on this engine's list**. Which engine's settings are `PRAGMA`s is the
capability `settings_are_pragmas`, which the form's hint asks too, never an engine comparison.
**`USE` is refused with every other non-`SET`**, because it would move each operation off the
database it asked for — the tab's, the tree node's — and an `INSERT` would be a write on every
ping.
**The lists name what may be set, not what may not.** The gate was `SET` plus a `DENIED` list
of the words a guard rests on, and every setting nobody had thought of went through. `SET
ANSI_DEFAULTS ON` turns `IMPLICIT_TRANSACTIONS` on, so on SQL Server every auto-commit write
rolled back at the disconnect under an `ok` report; `FMTONLY`, `NOEXEC` and `PARSEONLY` make
every statement a successful no-op; MySQL's `sql_select_limit` and T-SQL's `TEXTSIZE` left a
dump, copy or export silently short; `ANSI_WARNINGS OFF` stores an over-long value truncated
instead of failing the write; `QUOTED_IDENTIFIER OFF` makes `"x"` a string; a non-UTF-8 client
character set — `SET NAMES gbk` — moved where the server ends a string away from where
`skip_noncode` does, so the read-only text gate passed as one read what the server ran as more,
and MCP `run_query` deleted rows on a live server; and any denied name written quoted
(`` SET `autocommit` = 0 ``) went through, because the word scan skipped a quoted name as a
literal. A name on no list is refused now (`a_setting_nobody_has_weighed_is_refused`), so a
setting gets in only once someone has weighed it — the cost being that a harmless setting
nobody listed is refused until someone adds it.
`MYSQL_SETTINGS` is time zone, locale, timeouts, buffer sizes, optimiser knobs and an isolation
level (which cannot lift `SET SESSION TRANSACTION READ ONLY`). A MySQL user variable (`SET @x =
1`) passes, since it sets nothing Schemaic reads, with one literal value; the `SESSION` and `LOCAL` scopes, bare or as
`@@session.`, pass; `GLOBAL`, `PERSIST` and `PERSIST_ONLY`, bare or as `@@global.`, are refused
as changing the server rather than the session, and `SET PASSWORD` and `SET DEFAULT ROLE` fall
to the list, naming nothing on it. `POSTGRES_SETTINGS` is the same kind of list, plus the
planner's `enable_*` switches (`POSTGRES_SETTING_PREFIX`), any custom dotted `prefix.name` (an
application's or an extension's — the usual carrier of a row-level-security tenant) and `SET
TIME ZONE`. SQL Server's is three: `MSSQL_VALUE_SETTINGS` (`LANGUAGE`, `DATEFORMAT`,
`LOCK_TIMEOUT`, …), `MSSQL_SWITCHES`, which may go either way, and `MSSQL_ON_ONLY` —
`ANSI_NULLS`, `ANSI_PADDING`, `ANSI_WARNINGS`, `QUOTED_IDENTIFIER`, `ARITHABORT`,
`CONCAT_NULL_YIELDS_NULL`, the ISO switches whose default Schemaic's SQL is written for, which
pass only `ON`. A T-SQL `@x` is refused: a variable ends with its batch and sets nothing.
`SQLITE_PRAGMAS` lists the per-connection pragmas, unqualified and with an `=` value or none,
and leaves out what writes the file (`journal_mode`, `user_version`, `auto_vacuum`) or lifts
the read-only guard (`query_only`, `writable_schema`). **Left off on purpose**, each list's doc
comment saying which: the transaction characteristics and read-only flags, `autocommit`,
`IMPLICIT_TRANSACTIONS`/`ANSI_DEFAULTS`, `NOCOUNT`/`ROWCOUNT` (the row counts the one-row
safety net reads), `foreign_key_checks`, PostgreSQL's `session_replication_role` (it switches
triggers and foreign keys off) and how values the app reads are spelt — `bytea_output`,
`extra_float_digits`, `DateStyle`, `IntervalStyle` and `lc_monetary`, the first three pinned on
`pg::connect`'s startup packet, which an init running after it would undo (`DateStyle` and
`IntervalStyle` were listed, so a `SET DateStyle = 'SQL, DMY'` dumped 2 January as 1 February) —
and the settings the literals Schemaic writes are lexed by — PostgreSQL's
`standard_conforming_strings`, the one `pg::connect_probe` pins on the startup packet, and
MySQL's `sql_mode` (`what_the_guards_rest_on_cannot_be_moved`,
`settings_that_make_an_operation_deliver_less_are_refused`). The client character set is a
check of the value, not the name: MySQL's `SET NAMES` (with an optional `COLLATE utf8…`),
`CHARACTER SET` and `CHARSET`, and PostgreSQL's `NAMES` and `client_encoding`, pass with a
UTF-8 value only, and MySQL's `character_set_*` variables are on no list
(`a_client_character_set_other_than_utf8_is_refused`). `SET TRANSACTION` passes only as
`ISOLATION LEVEL` and a level named whole, and only where the read-only guard is a rollback
(`SqlDialect::read_only_is_a_rollback` — SQL Server), which an isolation level cannot lift;
elsewhere the transaction characteristics *are* the guard.
**A setting takes one value, and the statement ends there** (`one_value`, and `assigned` for
MySQL's `= value`): a word, a string (T-SQL's `N'…'` too) or a signed or fractional number, and
nothing after it — a MySQL user variable's value too. The arms used to return once they had
read a listed name, and **T-SQL needs no `;` between statements**, while `statement_ranges`
bounds them only at `;` and `GO`: `SET LANGUAGE us_english` with `SET ANSI_WARNINGS OFF` on the
next line was one statement to `check` and two to the server (confirmed on SQL Server 2022 —
the second `SET` takes effect), so every refused switch, or a `DELETE` on every ping — on a
read-only connection too, since its guard runs after the init — was one line-break away. On
MariaDB the value went unread the same way, and `SET @x = NEXT VALUE FOR s` advanced a sequence
on every operation (`a_setting_takes_one_value_and_nothing_follows_it`). PostgreSQL's arm keeps
its value lists (`search_path TO app, public`): it needs a `;` between statements.
**Each statement is tokenised, not word-scanned** — `tokens`, over `sql::statement_ranges` and
`skip_noncode`, the one boundary lexer, with a quoted name told from a string by
`intel::ident_quote` and **kept as a `Tok::Quoted` token rather than skipped**. Don't tidy that
token away: skipping a quoted name is exactly how `` `autocommit` `` got through, and a quoted
name is refused wherever a setting's name would stand, since it is the same setting to the
server and no setting needs the quotes (`a_quoted_setting_name_is_still_denied`). A word or a
parenthesis inside a string is only a value (`a_denied_word_inside_a_value_is_only_a_value`, and
`SET application_name = 'a (b)'` in `a_setting_that_calls_a_function_is_refused`), and the
settings people write keep passing (`everyday_session_settings_pass`). **The list is not the
last line, but the ordering is not the only one either.** A read-only connection's enforcement
statements run *after* the init SQL, inside each operation, so a transaction setting the list
missed is overridden there, and SQL Server's connect turns `IMPLICIT_TRANSACTIONS` off after
any init. But the lexing the text gate rests on is in place *before* enforcement runs, and no
enforcement statement re-sets the client character set — which is what `SET NAMES gbk`
showed. MySQL's `enforce_session` now reads `character_set_client` back; on PostgreSQL, where
tokio-postgres asks for `UTF8` on the startup packet and nothing reads it back, the list is
what keeps it there. Where and when the statements run is under `schemaic-db`'s init SQL
paragraph, beside the PostgreSQL probe.
