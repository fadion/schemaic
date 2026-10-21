# `cli/catalog.rs`

`cli/catalog.rs` — the canned SQL behind `schemaic tables` and `schemaic describe <table>`: two
pure builders, `tables_sql(dialect, database)` and `describe_sql(dialect, database, table)`, and
`DESCRIBE_COLUMNS`. **Text run through `query::read_only_query`, not a `Db` method of its
own**, because what they print is rows like any query's — so the gate, the read-only session,
the deadline and the cap are the one headless read path's, and `--format` means what it means
everywhere. Only the SQL is per dialect, because the question is: PostgreSQL and SQLite have no
`DESCRIBE`, and MySQL's `SHOW` names its columns after the database. So each builder is an
exhaustive `match` over the three, and every engine answers under the **same column names** —
`name`, `type` for `tables`, with `schema` first on PostgreSQL alone, the one engine whose
database holds more than one namespace; `column`, `type`, `nullable`, `default`, `key` for
`describe` — so a script reading one engine's output reads the others'. They are aliased through
`export::ident_sql`, since `default`, `key` and `column` are reserved words on at least one of
them (`every_engine_answers_under_the_same_column_names`), and a type is spelled one way
everywhere: `table`, `view`, and PostgreSQL's own `materialized view` and `foreign table`.
**MariaDB's `TABLE_TYPE` has more spellings than MySQL's**, and the listing used to pass any it
did not map through `LOWER(TABLE_TYPE)` — so a system-versioned table came out as `system
versioned`, and a script filtering `type == "table"` dropped every one of them. Now `SYSTEM
VERSIONED` maps to `table` and `SYSTEM VIEW` to `view`, and a `TABLE_TYPE IN (…)` filter
replaces the pass-through, which leaves MariaDB's `SEQUENCE` out as the PostgreSQL listing's
relkind filter leaves sequences out — no type reaches the output unmapped
(`a_mysql_listing_spells_every_type_one_way`).
**`None` means the dialect cannot answer without a database**, and `run.rs` says so before
anything is dialled. MySQL's reads filter on `DATABASE()` — the database the statement runs in,
which is the one `-d` named — and without one that is NULL and the listing is silently empty;
PostgreSQL's would list the maintenance database's tables as though they were the connection's.
A SQLite file always is a database, and it is always `main` — `"main".sqlite_master`,
`pragma_table_xinfo('t', 'main')` (`only_a_file_database_needs_no_database_named`). **`-d` and
`SCHEMAIC_DATABASE` are ignored there**, as `query` already ignored them: they used to be
spliced in as the schema (`"shop".sqlite_master`), which fails with exit 4 — "retry may work" —
on a command line that never could, and a `SCHEMAIC_DATABASE` exported for another engine did
the same. Every CLI operation opens its own connection, so nothing but `main` is ever attached
for a name to mean (`a_sqlite_listing_ignores_the_database_it_is_given`).
**PostgreSQL's listing reads `pg_class`, not `information_schema.tables`**, which has no
materialized views: relkinds `r`/`p`/`v`/`m`/`f`, partitions left out (`NOT relispartition` —
their parent is the table; the GUI's tree lists them, nested under it, under `schema_tree.rs`),
and `pg_catalog`, `information_schema`, `pg_toast*` and the per-session `pg_temp_*` schemas with them. Its `describe` finds the relation through
`pg_catalog.to_regclass(<literal>)`, which reads the name the way a query would — through the
search path when bare, `schema.table` when qualified, case-folded unless double-quoted — and a
name it cannot find is NULL, matching no row. **`to_regclass` also finds an index or a
sequence**, and one was printed as a table with exit 0, so the read joins `pg_class` and holds
the relation to `relkind IN ('r','p','v','m','f')`, the set of kinds `tables` lists
(`a_postgres_describe_is_of_a_listed_relation_only`). SQLite's
is `pragma_table_xinfo` as a table-valued function, because that makes it a `SELECT`, the only
head the read gate lets through (`every_canned_statement_passes_the_read_gate`). **`xinfo`, not
`pragma_table_info`**, because only `xinfo` reports a generated column (`hidden` 2 or 3) —
`describe` silently left them out, a different column set from the servers' with nothing to say
so (`a_sqlite_describe_lists_generated_columns`). `hidden = 1` is a virtual table's hidden
column, no column of its rows, and `WHERE hidden <> 1` drops it, as `db::sqlite`'s own column
read does. Both SQLite tests run the statement against in-memory SQLite, through a `rusqlite`
dev-dependency of this crate, since a pragma's column set is the engine's answer and not
something a string assertion can check. **What `key` can say differs per engine**,
always in MySQL's `COLUMN_KEY` words: MySQL gives its own `PRI`/`UNI`/`MUL`, PostgreSQL `PRI`
and `UNI` (a single-column unique index only, and not a partial one — `indpred IS NULL`, since
a unique index over some rows is no uniqueness of the column), and SQLite `PRI` alone, since
`pragma_table_xinfo` says nothing of unique indexes. **No rows means no such table** to `run.rs`; a PostgreSQL
relation with no columns at all (`CREATE TABLE t ()`) reads the same, and is accepted as rare
enough to take the wrong message.
**A name from the command line reaches the SQL only as a literal**, through
`export::sql_literal`, never spliced as an identifier — so a quote in it is doubled and cannot
end the string (`a_table_name_reaches_the_sql_only_as_a_literal`, whose `o'; DROP TABLE x; --`
still passes the gate as one read). The MySQL literal doubles a backslash, which is the default
`sql_mode`'s reading, and the read-only session holds the server to it: `mysql::enforce_session`
takes `NO_BACKSLASH_ESCAPES` out of the mode before the statement runs and refuses the
statement if the server puts it back. Verified live on MariaDB `classicmodels`,
PostgreSQL `chinook` and a scratch schema — a mixed-case quoted name, a view, a materialized
view, a partitioned table whose partition stayed hidden, and an injection-shaped name that came
back a plain miss — and SQLite through a throwaway `APPDATA` profile. The type mapping was
verified live on MariaDB 10.11 and the relkind and partial-index filters on PostgreSQL 16.
