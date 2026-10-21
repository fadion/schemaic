# `core/copy_table.rs`

`copy_table.rs` — **a table, structure and rows, into another database** of the same engine,
on this connection or another: the pure half of the tree's *Copy to* (the UI half is
`ui/copy_table_view.rs`, the rows `Db::copy_rows`). **Same engine only, and that is a non-goal
rather than a gap**: a copy across engines is a type mapping — `datetime` → `timestamp`, an
`enum` with no counterpart at all — and nothing in the workspace has one, so the picker offers
only the connections `connection::same_engine` matches. That predicate counts MySQL and MariaDB
as one engine, so a copy between them is offered and nothing here translates a type or a table
option one server has and the other lacks.
`CopyTarget { database, schema, name }` is where it goes. `refusal(src, to, same_database)`
answers a sentence or `None`: no name, no database, or — when the caller says the target is
the source's own connection and database — the source's own name in the source's own namespace,
compared ASCII-case-insensitively, which would be a copy onto itself.
**The copy is a new table, and `draft` is the one place that makes it one.** It is
`TableDraft::from_table` with `original = None` and every column, index and check rebuilt
through its `::new`, so `ddl::create` treats nothing as already there; `plan` is exactly
`ddl::create(&draft(…))`, which is what keeps the copy on the DDL invariant's one path — the
preview shows it and `Db::run_ddl` runs it like any other plan
(`the_plan_creates_the_copy_and_never_names_the_source`). **Foreign keys are cleared**: the
tables they reference need not exist on the target, and a key naming a missing table refuses
the whole `CREATE`. **Triggers do not come**, a draft not carrying them. **A copy is one plain
table**, whatever the source's partitioning, on every engine by choice: `partitioning` and
`partition_of` are cleared. The reasons are PostgreSQL's — a partitioned copy there has no
partition for any row it then inserts, and a partition copied `PARTITION OF` names a parent the
target may not have — while the rows a partitioned table reads are its partitions', so the
plain copy holds them all; a MySQL copy, which could have declared its partitions, simply keeps
none of the source's partitioning. An index the source
inherited from its parent (`IndexInfo::inherited`) becomes the copy's own, there being no
parent to give it one (`a_partitioned_table_or_a_partition_is_copied_as_a_plain_table`).
**An index's and a
check's name move onto the copy's** (`moved_name`), because index names are per schema on
PostgreSQL and per database on SQLite and a check's per schema on MySQL and SQL Server, so a copy
beside its source under the source's index names would collide on every one: where the name
holds the table's it is substituted (`orders_total_idx` → `orders_copy_total_idx`), elsewhere
the copy's name is put in front (`an_index_and_a_check_take_the_copys_name`). A copy that keeps
the table's name keeps every name (`a_copy_under_the_same_name_keeps_every_name`) — it is in
another database, or `refusal` stopped it — and SQLite's `sqlite_autoindex_*` stay as they are,
being the engine's, re-declared as `UNIQUE` rather than created by name
(`sqlites_own_indexes_are_not_renamed`). The substitution is a plain case-sensitive `find`.
`columns(src, dialect)` is every column but those `ColumnInfo::is_server_assigned` names — the
import's own rule, so a generated column is neither read nor written and is computed again on
the target (`a_generated_column_is_not_read_or_written`) — **except an identity the write can
carry** (`carries_identity`), because its values are the table's keys. That is SQL Server's,
`dump::carries_identity` as the dump asks it, written under the import's `IDENTITY_INSERT`; and
PostgreSQL's `GENERATED ALWAYS`, written through `INSERT … OVERRIDING SYSTEM VALUE`
(`import::overriding_clause`, `Some` on PostgreSQL alone) and its counter then settled like a
`BY DEFAULT` one's. Both used to be left out to take fresh values, and SQL Server reads *every*
identity as `identity_always`, so the copy renumbered `1..n` in whatever order the read
streamed under a success report — ids 2 and 4 came out 1 and 2 on a live server (per the
change's author). A `rowversion` is `identity_always` with no `auto_increment` and stays the
server's (`an_always_identity_is_copied_with_its_values`, `a_rowversion_is_not_copied`).
**The dump keeps its own rule**: it still renumbers a PostgreSQL `GENERATED ALWAYS` identity,
its shared renderer having no way to write the clause (under `dump.rs`), so the two differ on
exactly that column.
**`rows_read(database, src, dialect)` is everything the rows half needs** — `RowsRead { columns,
forms, select, overriding }`, `overriding` being whether a carried identity needs the clause —
and the UI's `CopyRowsRequest` carries it to `db::ImportTarget`. **A raw-bytes value is read as
text the server makes of it, never as the cell the grid shows.** Every engine's streamed read
renders one as `model::binary_display`'s `<n bytes>`, and the copy wrote that placeholder into
the target as the value: SQL Server refused it (Msg 257), the other three stored the nine
characters. `read_form(column, dialect)` picks the expression and the `import::CellForm` it
arrives in: MySQL/MariaDB binary and spatial types as `HEX(col)` and PostgreSQL `bytea` as
`encode(col, 'hex')` (`Hex`); SQL Server through the dump's own `dump::literal_select`
(`Server` — `varbinary`/`image`/CLR types as `0x…`, a `sql_variant` as its base type); and on
SQLite **every** column as `quote(col)` (`Quoted`), since a declared type there is only an
affinity and a blob can sit in any column — `quote()` is lossless for every storage class,
±infinity included. **A MySQL-family `FLOAT` is read as the double it is** —
`dump::exact_float_select`, `` (`x` + 0e0) AS `x` ``, which the dump's rows read too: the text
protocol spells a `FLOAT` with six significant digits, so `16777215` copied as `16777200` and
`1.2345678` as `1.23457`, other floats, while as a double it arrives exact and the `FLOAT`
column takes it back as the same float (measured on MariaDB 10.11 and MySQL 8.4;
`a_mysql_float_is_read_as_a_double`, `a_mysql_float_column_is_read_as_a_double`).
**`rows_read_with`/`read_form_with` take the source's SQL Server alias types**
(`DbSchema::tsql_objects`, which the modal passes from the loaded schema), so a column typed
`[dbo].[Hash]` over `varbinary` is read by its base as the dump reads it, where with none it
stopped every copy of its table and an alias over `sql_variant` came back an `nvarchar` variant
(`a_sql_server_alias_type_is_read_by_its_base`). Everything else is `Value`. `select_sql` is the read on the source: those
columns in table order, each as `read_form` reads it, qualified by `export::qualified_table`
and unsorted, since an insert needs no order
(`a_blob_is_read_as_text_the_write_turns_back_into_bytes`; SQLite's
`a_copy_carries_a_blob_byte_for_byte`).
**`chunk_refusal` is the net under `read_form`**, asked by `db`'s `ChunkRows` at the first row
of every streamed chunk: it fails the copy when a cell in a bytes column (`binary_columns` or
`Column::is_binary`) is still a placeholder (`model::is_binary_display`), or when any column hit
the chunk's arena cap (`capped_columns`), whose cells read blank past it. A type `read_form`
does not know — an alias type the schema was not loaded with — therefore stops the copy
loudly instead of writing the placeholder
(`a_chunk_holding_a_bytes_placeholder_is_refused`, which also holds that prose reading
`<3 bytes>` in a text column passes; SQLite's `a_copy_never_writes_a_bytes_placeholder`).
It answers a **`CopyStop`** rather than a sentence — `TooLong { column }` (`None` when the read
did not name it) or `Bytes { column }`, with `NotItsForm { column }` the third arm,
`import::build_insert_as`'s refusal of a cell not in its form's shape — so the interface can
word why a copy stopped; `message()` is the English the `String` said before, byte for byte
(`a_copy_refusal_says_what_it_used_to`). It is a *stop* because `CopyRefusal` was taken: that
is `refusal`'s answer, the modal's own refusals before anything runs (*Name the copy.*), this a
value met mid-write, which reaches the interface inside `import::RowError::Copy`.
`rows_change(src)` is the English of the preview's change-list line for the rows, which the
modal words itself (`rows_line_text`, under `copy_table_view.rs`) — *"Then copy every row of
`<source>` into it (not shown as SQL)"*, the source through `schema::display_name`, so bare
where it has no schema. The rows are no statement in the plan: `Db::copy_rows` streams them once
the `CREATE` has committed, so the preview listed only the `CREATE`'s own line and read as a
structure-only copy while Apply went on to write every row
(`the_rows_step_names_its_source_and_says_it_is_not_in_the_sql`,
`the_rows_step_names_a_schemaless_source_bare`).
**A copied counter is the engine's question, asked as a capability.**
`counter_follows_written_values(dialect)` is whether an explicit value written into an
auto-numbered column moves the engine's counter past it: MySQL's `AUTO_INCREMENT`, SQL Server's
identity under the import's `IDENTITY_INSERT` and SQLite's rowid do; a PostgreSQL identity's
sequence does not, and would hand out `1` next on a table that already holds it. `settle_sql` is
what runs on the target after the rows where it is `false` — per auto-increment column whose
values were copied, a carried `GENERATED ALWAYS` one included,
`SELECT setval(pg_get_serial_sequence('<qualified>', '<col>'),
COALESCE(MAX(col), 0) + 1, false) FROM <qualified>`, the table name quoted inside the string so
case and schema survive `pg_get_serial_sequence`'s parse, and `is_called = false` so the next
value is exactly `MAX + 1`, and `1` on an empty table
(`only_postgres_settles_its_counter_after_the_rows`). The live tier's
`a_copied_table_has_every_row_and_numbers_on_past_them` is what proves it moved: it seeds ids
1, 3 and 4 — the gap is what shows they were copied rather than renumbered — beside a blob and
the floats each engine can hold (PostgreSQL's NaN and infinities, SQLite's infinities), asserts
the copy's rows equal the source's with the bytes compared as hex, settles, and inserts one more
row expecting id 5; with PostgreSQL's settle skipped it fails on a duplicate key at 1 (per the
change's author, measured when the seed was ids 1–3).
