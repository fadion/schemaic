# `core/skeleton.rs`

`skeleton.rs` — the `INSERT`/`UPDATE`/`DELETE` drafts behind a table's **Generate** menu, and
they are drafts *for a person*, never statements for a server. Values are named placeholders
(`:price`), which no engine accepts, so a skeleton run by reflex fails to parse instead of
writing a row of empty strings or updating every row in the table; `a_value_is_never_a_literal`
is the test that pins it. **The slots are uniquified within a statement** (`placeholders`, over
the per-column `placeholder`): the flattening that turns a non-word character into `_` made
`first name` and `first_name` — the spreadsheet-import shape it exists for — both yield
`:first_name`, and every column with nothing word-like in its name yield the one `:value`, so
filling the draft in set two columns from one typed value with nothing in the text saying so.
The rule is `export::export_json`'s for duplicate result columns — first occurrence keeps the
bare slot, the rest take `_2`, `_3`, … — and the suffix is bumped until it is free rather than
assumed to be, since a table holding `first name`, `first_name` *and* `first_name_2` would
otherwise collide on the fix. Nothing about what a statement addresses is invented here — the name
is `filter::qualified_table_name` and the `WHERE` is `schema::browse_key_columns`, the key the
grid's write-back already addresses a row with — and a table with **no** key gets a `WHERE`
that names the problem and *doesn't parse*, because the alternative is a statement that runs
against every row. Which columns are named is one step stricter than
`ColumnInfo::is_server_assigned` (the rule real write paths obey, "the server rejects a value")
and drops `AUTO_INCREMENT` keys too, which accept one but which nobody hand-writes; when that
leaves nothing, the columns the server at least *accepts* a value for come back (the
numbered-key-only table), and only failing that every column, since a statement naming no
columns is not a draft of anything. The middle stage is the load-bearing one: collapsing
straight to every column named the `is_server_assigned` ones, which the server refuses however
the draft is filled in (`the_fallback_never_names_a_column_the_server_refuses`). `skeleton_columns` is
`pub(crate)` because the editor's `INSERT INTO t (` ghost (`intel::predict`) names the same
columns, so the two never disagree about what an `INSERT` writes. The `CREATE` in that menu is not from here — it is `DbSchema`'s own
`create_ddl_script`, which emits real DDL. **Whatever generates it, the tab it opens in is bound
to the database the statement is *for*** — `TabActions::open_query` takes it as a parameter, and
every schema-menu entry, plus the DDL preview's "Open in editor", passes the node's own. Falling
through to `default_tab_target` is what a *new* tab does (the last database picked, else the
connection's first by name), and it opened `employees.employees`'s DDL bound to `bigschema` — a
toolbar contradicting the SQL beneath it, one Ctrl+Enter from building those tables in the wrong
database. `None` still means "no particular database", which is what a free-standing snippet is;
the AI panel's code blocks pass the *active tab's*, since the conversation is about that tab.
