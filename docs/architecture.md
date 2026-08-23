# Schemaic — architecture

A native SQL editor (Rust + [Floem](https://github.com/lapce/floem) 0.2.0), MySQL/MariaDB-first,
Zed-inspired, aiming to replace DataGrip. PostgreSQL and SQLite are wired too; SQLite is
read/write and edits **tables** (through the twelve-step rebuild), **views** and **triggers**, but
has no manual-transaction mode — see `db::session`'s `Session::open` for what that one is a
statement about. All three engines now edit all three of those objects, and they get there
differently, so ask the *narrow* capability (`ddl::supports_or_replace_view`,
`ddl::supports_view_rename`) rather than the engine.

This is the project's reference document: the crate/module map, the architecture invariants, the
UI conventions, and the Floem hazards each subsystem is built on. `CLAUDE.md` at the repo root
holds the *working* rules — how to build, test and commit — and points here for everything else.
Keep the two disjoint: an instruction to the person or agent doing the work belongs there, a fact
about the system belongs here.

**Prefer reading this through a subagent.** It is ~3.4k lines; paging it into a session wholesale
is what runs that session out of context. `scout` (in `.claude/agents/`) answers "where/how"
questions against this document *and* the code and reports back a conclusion rather than a
transcript, and `arch-scribe` makes the edits a finished change requires. Read a section here by
hand when you are about to edit it, or when a citation is not enough.

**It is only worth this much if it stays true.** Silent drift from the code is the most damaging
kind of bug in a document everyone trusts — `core/tests/doc_coverage.rs` catches a module that was
never written down, and nothing catches a paragraph that has quietly become false. When a change
lands, route the write through `arch-scribe` rather than leaving it for afterwards.

## Contents

- [Crates](#crates) — the module map, one entry per source file
- [Architecture invariants](#architecture-invariants-dont-regress-these) — the rules re-introducing
  which is a regression
- [UI conventions](#ui-conventions) — cursors, labels, colours, theming
- [Floem 0.2 gotchas](#floem-02-gotchas-learned-the-hard-way) — the framework hazards, learned the
  hard way (focus, Tab, scroll, overlays, transitions)
- [Popup menus](#popup-menus-menu_panel) — `menu_panel`, submenus, dismissal, edge-flipping
- [Data grid](#data-grid-results-grid) — the results grid, top to bottom

## Crates

- `schemaic-core` — models, persisted UI state (`persist.rs`), transcript types, and the pure,
  unit-tested SQL/edit/export logic (UI/app keep thin wrappers; regression tests live here):
  - `model.rs` — the shared result-set model everything else is phrased in. `Value` holds text-
    protocol cells parsed into compact numeric variants *only where lossless* (`DECIMAL`/dates/JSON
    stay `Str`, so nothing is rounded or reformatted); `ResultSet` stores them columnar, so
    `CellRef` reads a cell without allocating, and **each column sits behind its own `Arc`** — the
    grid's `rs` signal and the tab's canonical `QueryState::Loaded` hold the *same*
    `Arc<ResultSet>` on purpose, so the post-commit splice mutates through `Arc::make_mut` at a
    strong count of 2; with the columns inline that deep-copied every arena (30 ms and ~160 MB at
    200k×50, on the UI thread, on the one path built to avoid a rebuild). `splice_rows` replaces
    only the column `Arc`s whose values actually changed, so an untouched column is never copied
    (29.6 ms → 1.8 ms at 200k×50, measured). `Column`/`ColumnOrigin`/`ColumnFlags` carry the
    write-back provenance the wire reports per column, and a binary column is unconditionally
    read-only (it can't round-trip through text). **A raw-bytes cell has exactly one rendering, and
    it lives here:** `binary_display(len)` → `<n bytes>`, with `is_binary_display` as its
    recognizer and `type_is_binary` / `Column::is_binary` as the question "is this column bytes at
    all". The three engines each used to answer differently — SQLite showed the size, MySQL
    `from_utf8_lossy`'d the bytes into mojibake, PostgreSQL handed over the text protocol's `\x…` —
    and the mojibake was a data bug rather than a cosmetic one: it *looks* like data, so a CSV or
    `INSERT` export wrote the replacement characters as the value and re-imported as the wrong
    bytes. SQLite's was the honest answer and is now everyone's. `Column::is_binary` reads **two**
    inputs because neither covers every result: `ColumnOrigin::binary` is the authoritative wire
    flag but exists only for a table-backed column, so a `bytea` expression with no catalog
    provenance reached every caller as ordinary text until the type name was consulted too.
    Conversely nothing may act on the type name *alone* — a SQLite `BLOB` column is an affinity,
    not a promise, and may hold ordinary text — which is why every decision that discards a value
    (`export::dropped_binary_columns`, `pg::pg_cell`) requires the type and the value to agree.
    **The flag is computed once per result, never per cell.** `Column::is_binary` splits a type
    name and walks a keyword list; the read loops run up to the row cap times the column count, so
    both backends hoist it out — MySQL into a `Vec<bool>` before `ResultBuilder::new`, PostgreSQL
    into `pg::binary_columns` riding alongside the per-column type names in the `grid` tuple. Asked
    per cell it would be tens of millions of string splits for an answer that cannot change between
    rows.
    `ColumnOrigin::implicit_key` is the one field no
    wire reports: it marks a result column that identifies a row but is no column of the table —
    SQLite's explicitly projected `rowid` — asserted by the backend on the same trust
    `ColumnFlags::primary_key` already carries, and `false` on MySQL and PostgreSQL. It is a key
    that is never *editable*, which is why it is a flag and not simply another key column. The
    write path's shared decisions live here so the
    two engines can't drift: `GridWrite::plan` (the deletes → updates → inserts order),
    `one_row_verdict` (the 1-row safety net's verdict *and* its message),
    `Rollback`/`engine_is_transactional` (what a rollback actually achieved — see the invariant
    below), and `drop_committed` (which staged edits a commit un-stages).
  - `aggregate.rs` — what a multi-cell grid selection adds up to (`aggregate` → `Aggregates` +
    `summary`). The arithmetic is **fixed-point, not `f64`**, and that is the whole reason the
    module has substance: `Column::is_numeric` counts `DECIMAL`/`NUMERIC`, while `Value` leaves
    those cells as `Str` precisely so the wire's digits are never rounded — and a money column is
    exactly what anyone wants a `Sum` for. Summing through a float would reintroduce, in the one
    number the user reads, the error the storage model exists to avoid (`45.599999999999994` under
    a column of tidy prices). So values parse into `Fixed` (an `i128` of units at a scale), sum and
    compare at the widest scale present, and format back keeping the column's own decimals; only
    the average divides, and it carries extra places to say so. Overflow degrades to *no* aggregate
    rather than wrapping — a silently wrong total is worse than an absent one. NULLs are counted
    but excluded (the average divides by what it actually had), a numeric cell that doesn't parse
    is present-but-skipped rather than treated as zero, and a non-numeric column gets counts only:
    there is nothing to sum in a name, and a lexicographic min/max reads as a bug more often than
    it answers anything.
    **There are four overflow sites and they do not degrade alike**, which is how three different
    answers to one question shipped; each is tested apart. `Fixed::parse` answers
    `Parsed::{Value, Overflow, NotANumber}` — three, not two, because a number too wide returning
    the same `None` as `"n/a"` made `aggregate` *skip* it, and a `double` column holding `1e39` and
    `2` then reported `Sum 2`: the one path in the app that printed a number simply untrue
    (`Value::Float` stores `f64::to_string()`, which never uses exponent form, so forty digits
    really arrive). `rescale` overflows on scale **spread** alone and is left as a degrade
    deliberately — capping the working scale would make it the module's first rounding decision.
    The running `checked_add` is the third. The fourth is the average, and it alone is an `Option`
    on `NumericAggregates`: carried as a plain `Fixed` it propagated `?` and withheld an exact Sum,
    Min and Max because the *mean* of them didn't fit — and it divides before it scales, so the
    `DECIMAL(38,10)` case `Fixed`'s own headroom paragraph calls comfortable is representable.
    `aggregate_texts` is the entry point the grid uses: a staged (green) edit and a pending new row
    are both on screen and neither is in the `ResultSet`, and a total under an edit it doesn't
    include is the same defect as a stale one. The fold keeps no `Vec` — it raises its own
    accumulators when it meets a wider scale — because the vector was 32 bytes per selected cell,
    allocated and freed on **every** recompute (6.4 MB for a Ctrl+A over 200k rows, once per
    auto-repeat and once per cell crossed in a drag).
  - `sql.rs` — one `skip_noncode` tokenizer → statement splitting, unsafe-statement guard, AI
    read-only gate, `edit_distance`. The *single* SQL boundary lexer; `intel` (scope/context/
    diagnostics)/`sql_highlight`/`sqlfmt` all build on it so string/`#`/`--`/`/* */`/backtick
    boundaries agree by construction.
    **The per-dialect rules are a capability table on `SqlDialect`**, one predicate per divergence
    (`dash_comment_needs_space`, `hash_line_comment`, `backslash_escapes`, `e_string_backslash`,
    `double_quote_is_ident`, `backtick_ident`, `bracket_ident`, `dollar_quoted`,
    `delimiter_directive`), and they are predicates because the question stopped being binary. The
    scanner used to ask `dialect == Postgres` / `!= MySql`, which silently sorts any *third* engine
    onto whichever side each comparison happens to put it — with nothing failing to compile, since
    `!=` is exhaustive over any number of variants. Three of those defaults were wrong for SQLite
    and two dangerously: it has no `\` escape inside a string, so `'C:\'` under MySQL's rule latches
    the scanner into a literal that never ends and swallows the rest of the statement, which is
    exactly how a `WHERE` gets hidden from the guard this module was consolidated to kill. Adding an
    engine now fills in the table rather than hoping a `!=` falls the right way. `comment_open` is
    the classification half, exposed because `pairs::region_at` has to tell a comment span from a
    string span after the lexer has found one and was answering it with its own byte test.
    **The AI read-only gate's allowed heads are a per-dialect list too** — `read_only_heads`, which
    `read_only_reason` both tests against and builds its rejection message from:
    `SELECT/SHOW/DESCRIBE/DESC/EXPLAIN/WITH` on MySQL, `SELECT/SHOW/EXPLAIN/WITH` on PostgreSQL
    (`SHOW search_path` is real SQL there, while `DESCRIBE` isn't — psql's `\d` is a client command
    rather than a statement), `SELECT/EXPLAIN/WITH` on SQLite. One shared list was wrong in both
    directions at once for a third engine: it waved `SHOW TABLES` through to SQLite, which has no
    such syntax, so the model got a raw parser error instead of being told the engine has no such
    thing, and a rejection named heads that connection couldn't use — which a model will keep
    retrying. It is also what the MCP server builds `run_query`'s advertised description from, so
    the tool text and the gate can't drift
    (`the_read_only_heads_are_the_ones_the_engine_actually_has`,
    `the_rejection_lists_only_this_engines_heads`). Only the head list is per dialect: the
    single-statement check and the `DENY_KEYWORDS` scan — which is what refuses a write hidden
    behind a `WITH` head — apply the same everywhere.
    **The gate's *lexer* half is per dialect as well, and is where the real bypass was.** Whether a
    statement has ended is a dialect question, so the same text gets different — and individually
    correct — verdicts: `SELECT 'a\' ; DELETE FROM s; --'` is one statement on MySQL, whose
    backslash escape swallows the rest of the literal, and two anywhere else. A SQLite connection
    gated with `SqlDialect::MySql` therefore passed a payload that deletes a table (`c6c5dae`).
    The tests read `EVERY_DIALECT` rather than the module's MySQL-binding helper, and three
    (`the_gate_reads_this_engines_string_escape` / `…_comment_rule` / `…_identifier_quoting`) assert
    that the engines' answers *differ*, so a mis-paired dialect fails instead of passing by
    coincidence.
  - `intel.rs` — the **SQL intelligence** layer (structure-aware, dialect-pluggable). Parses a
    *complete* statement with a real per-dialect AST (`sqlparser`; `SqlDialect` seam — MySQL,
    PostgreSQL and SQLite all wired) and answers what a token stream can't: `statement_scope`
    (tables/aliases/CTEs/derived-tables in scope, AST-backed with a `skip_noncode` lexer fallback for
    mid-edit), `clause_context`/`clause_continuation` (caret context + expected-token model for
    completion), and `diagnostics` → `Vec<Diagnostic>` (catalog-aware unknown-table, unknown-column via
    the per-scope resolver `colres` — qualified *and* unqualified, across subqueries/derived-tables/CTEs
    with correlation — reserved-keyword-alias errors, syntax errors on completed statements, and
    keyword-typo warnings). **AST for classification, `skip_noncode` for byte positions by default** —
    except `colres`, which uses sqlparser 0.62's now-accurate per-identifier *spans* (verified) so the
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
    misses by construction and there is no write site to remember. Hosts the shared `SQL_KEYWORDS`/`SQL_FUNCTIONS`/
    `STMT_KEYWORDS` (the UI's completion + editor build on these). Also `join_condition` (FK-aware
    `JOIN … ON` auto-fill), `db_error_diagnostic` (positions a live DB error within the statement),
    `parses` (Tier-2 gate), and `select_output_names` (a projection's column names *in order*, or
    `None` when the statement alone can't say — what `ddl::pg_replaceable` reads).
    **`simple_select_source` is the one definition of "structurally simple enough to aim a write
    at"** — one statement, a `SELECT` body, no CTE, one `FROM` entry, no joins, a plain named table
    — shared by `filter::build_query` (which needs to know it may splice a `WHERE` into the
    statement that produced a result) and by SQLite's write-back (which needs to know which base
    table a grid row belongs to). Two predicates that agreed on the day they were written is the
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
    — anything that isn't a plain lower-case word that can stand unquoted as an *identifier*
    (`must_quote_ident`, below). PostgreSQL folds an unquoted
    `ArtistId` to `artistid`, so unquoted output couldn't run on any mixed-case schema, while
    unconditional quoting would backtick every ordinary MySQL name in text the user is about to
    edit. `JoinTarget` therefore carries the **bare** name for the popup to display and prefix-match
    and `table_sql` for what is actually inserted.
    **A reserved word is two questions, and they carry opposite costs.** `is_reserved_word` (over
    `MYSQL_RESERVED` / `PG_RESERVED` / `SQLITE_RESERVED`) asks whether a word can be a bare
    **alias**, and backs the nine diagnostic sites here — the alias check, the scope's alias
    resolution, the botched-alias warning — where listing a word wrongly squiggles working SQL, so
    those lists deliberately lean towards missing one. `must_quote_ident` asks whether it can be a
    bare **identifier**, a table or column name, which is the question a quoter has: miss a word
    there and the SQL emitted does not parse. On MySQL and PostgreSQL the two coincide, so
    `alias_ok_but_unquotable` is empty for both — a reserved word is reserved everywhere and one
    list answers both. SQLite is where they come apart, because its parser falls back to treating
    most of its ~147 keywords as identifiers wherever the grammar allows one: `CAST`, `IF` and
    `RAISE` are refused as a bare name yet are perfectly good `AS` aliases. Until the two were
    split, `export::ident_if_needed` and `filter::needs_quoting` were asking the **alias** set, so
    `filter::table_query` over a table `if` keyed on a `cast` column generated
    `SELECT * FROM if ORDER BY cast ASC LIMIT 10` and SQLite answered `near "ASC": syntax error`.
    Neither list is a transcription of anybody's documentation any more: `db::sqlite`'s
    `the_reserved_lists_match_what_sqlite_itself_refuses` walks the engine's own keyword table
    (`sqlite3_keyword_count`/`sqlite3_keyword_name`) and compiles every keyword on a real in-memory
    connection in three positions — bare column name, bare table name, bare `AS` alias — asserting
    both predicates against what SQLite actually accepts. It is a standing guard rather than a
    snapshot, so a keyword a future release adds arrives on its own and fails. Against 3.46.0's 147
    keywords it found the 57 existing `SQLITE_RESERVED` entries all correct and exactly four words
    missing from the identifier set: the three above, plus `NOTHING` (from `ON CONFLICT DO
    NOTHING`), which is refused in every position and so joined `SQLITE_RESERVED` itself.
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
  - `filter.rs` — the header filter/sort bar: a dialect-aware `sqlparser` **AST rewrite** that
    splices a `WHERE`/`ORDER BY` into the `SELECT` that produced the result and hands back SQL to
    re-run — so filtering covers the whole table, not the loaded page. `build_query` rewrites only
    a structurally simple, join-free, CTE-free single-table `SELECT` and degrades to `Ok(None)`
    ("not filterable") rather than erroring, because eligibility is the caller's question — a
    question it now asks `intel::simple_select_source`, which is also what SQLite's write-back
    asks, rather than answering inline.
    `eq_condition` is the right-click "Filter by / Exclude" fragment. `table_query` is what opening
    a table from the tree generates: it orders by the PK on purpose — no engine promises row
    order for a capped page, and PG heap order shifts under an `UPDATE` — while leaving an ordinary
    name unquoted, since a quoted identifier blinds the mid-edit tokenizer behind completion. It
    names a SQLite table **alone**: a connection *is* one file, so there is no server-level
    qualifier to add, and `main.t` would be noise on every generated query and wrong the moment the
    statement is copied somewhere the file is attached under another name.
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
    `is_reserved_word`, which answers the alias one and on SQLite is a shorter list.
    The table name itself is **`qualified_table_name`**, which `table_query` and `skeleton.rs` both
    call: whether a dialect needs a qualifier at all is a capability answered once, or a generated
    `UPDATE` and the browse `SELECT` above it end up spelling the same table two ways.
  - `skeleton.rs` — the `INSERT`/`UPDATE`/`DELETE` drafts behind a table's **Generate** menu, and
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
    leaves nothing at all, every column comes back, since a statement naming no columns is not a
    draft of anything. The `CREATE` in that menu is not from here — it is `DbSchema`'s own
    `create_ddl_script`, which emits real DDL. **Whatever generates it, the tab it opens in is bound
    to the database the statement is *for*** — `TabActions::open_query` takes it as a parameter, and
    every schema-menu entry, plus the DDL preview's "Open in editor", passes the node's own. Falling
    through to `default_tab_target` is what a *new* tab does (the last database picked, else the
    connection's first by name), and it opened `employees.employees`'s DDL bound to `bigschema` — a
    toolbar contradicting the SQL beneath it, one Ctrl+Enter from building those tables in the wrong
    database. `None` still means "no particular database", which is what a free-standing snippet is;
    the AI panel's code blocks pass the *active tab's*, since the conversation is about that tab.
  - `edit.rs` — `analyze_edit` → `EditModel` (write-back updatability analysis) + `refetch_template`
    and `refetch_key`, the **one** post-edit re-fetch key builder. A key column *is* editable
    (`EditModel::editable` asks only whether a column maps to a base table), so the `UPDATE` keys on
    the original row while the re-fetch must look for the value it just wrote; there were two
    builders and only one knew that.
    **The one key column that is never editable is an implicit key** (`ColumnOrigin::implicit_key`).
    `resolve_key` falls back to it only after a primary key and a fully-present unique NOT NULL
    index have *both* failed — a real key is what the user means by the row's identity, and it is
    what survives a re-fetch, where a rowid the engine may reassign is not — and `analyze_edit` then
    leaves that column out of `col_table` altogether: it is the handle the write holds the row by,
    not data the table has, and a newly inserted row has no value to offer for it. That exclusion is
    also what keeps it out of a staged `RowInsert`, with no second rule to remember. Tested by
    `keyless_table_is_editable_through_its_implicit_key`,
    `a_real_key_still_wins_over_a_projected_implicit_one` and
    `refetch_template_keys_on_the_implicit_key` — the last because a read-only key column is still
    part of the row the splice re-reads.
    **Three grid gestures resolve to a decision here rather than in the view.**
    `selected_data_rows(order, selection, pos)` is what a gutter gesture acts on: the highlighted
    range when the click landed inside it, otherwise the row it pointed at alone, mapped through the
    grid's display→data `order` and with pending new rows (which live past `order.len()`) left out.
    It is pure because it decides *which rows are deleted* — on a sorted grid the display index and
    the data index are different numbers, and the write-back's 1-row net checks the count and not the
    identity, so an inverted mapping deletes the wrong row and reports success. `copy_scope` answers
    what a cell menu's Copy takes — `CopyScope::Selection` for a right-click inside a multi-cell
    block (which no longer collapses the selection, so the menu is about the block),
    `CopyScope::Cell` otherwise — and `CopyScope::label` gives the two amounts two words, because an
    entry reading "Copy" that took one cell out of nine said what Ctrl+C and the gutter menu's own
    Copy say for three different amounts. `attach_span(r0, r1, cap)` returns *sent* and *selected*
    as two numbers: `prompt::ATTACH_ROW_CAP` is about the context window rather than about consent,
    so exceeding it is reported in the header instead of silently applied, and a figure derived from
    the rows that survived would tell a user who picked 900 that 200 went.
    **And the surfaces that *read* the grid resolve a cell here rather than in the view.**
    `GridCells` is a borrow struct over what the grid's signals hold — `rs`, the display→data
    `order`, the per-column `formats`, `dirty` and `new_rows` — and `text(i, ci, formatted)`
    resolves one *display* cell in the painter's order: a pending new row's typed value, then a
    staged edit, then the stored cell through `format::apply`. `tsv(rect)` is the clipboard's block
    and `attached(rect, cap)` is an AI attachment's column names, rows and pre-cap total. One rule,
    because this resolution kept going out one source short where nothing could test it:
    `attached_rows` first read `rs.cell` and never `dirty`, so a green uncommitted edit was on
    screen while the pre-edit value went to the model, and the fix for *that* left the rule in
    `grid.rs`, where it went short again — no `format::apply`, so a `Timestamp` column sent
    `1709294400` where the grid showed `2024-03-01 12:00:00`, with the sent-attachment card
    agreeing with the wrong copy because it is built from the same rows (`Bytes`, `Grouped` and
    `Bool` columns diverged the same way). `grid.rs`'s `copy_selection` and `attached_rows` are now
    the signal reads and nothing else; its `displayed_cell_text`/`pending_cell_text` are gone.
    **The painter is deliberately not a caller.** `data_cell`'s content `dyn_container` runs per
    cell per frame and reads the signals one at a time, so it stays the reference implementation
    and `GridCells::text` is written to match it — change one and read the other. `formatted` is a
    parameter because the two readers differ on purpose: an attachment passes `true`, its whole
    promise being that the model is answering about what the user is looking at, while Ctrl+C stays
    raw and the cell menu offers *Copy formatted* as its own entry. A staged value is never
    formatted either way, because it is text the user typed and the painter doesn't format one.
  - `export.rs` — CSV/JSON/SQL/Markdown/HTML export (incl. CSV formula-injection guard;
    Markdown pipe/backslash escaping; HTML entity escaping). Every renderer has a **streaming**
    `*_to<W: io::Write>` form (`ExportFormat::render_to`) — what file export uses, so a large
    result is never rendered into a second full copy in memory — with the `String` versions kept
    as thin wrappers for the clipboard. A test asserts the two agree byte-for-byte per format;
    add new formats to both by adding the `*_to` and wrapping it. **The SQL export is the one
    renderer that must not pass a cell straight through.** A raw-bytes cell is
    `model::binary_display`'s `<n bytes>` (a `Value` has no bytes variant to hold the real thing),
    and quoting that into an `INSERT` produces a script which silently stores the *placeholder* as
    the column's data on re-import. `dropped_binary_columns` finds those cells in a pre-pass —
    requiring the column's type **and** the cell's text to agree, since either signal alone is
    wrong in a way that loses data — writes `NULL` in their place, and heads the script with a
    `-- NOTE:` naming the columns. A comment rather than a refusal: the script still runs, and the
    one thing it may not do is pretend the placeholder was the data. The note is emitted only when
    a cell was actually dropped. The human-readable formats (CSV/JSON/Markdown/HTML) keep the
    placeholder — it is what the grid shows, and it cannot be mistaken for a value.
  - `import.rs` — the inverse of `export.rs`: CSV/TSV + JSON (array *or* NDJSON) → table. Format
    inference, delimiter/header `sniff` (by *consistency*, quote-aware), `auto_map` (name-match with
    a header, positional without), per-column `coerce` (only the families a wrong answer would
    definitely break — int/uint/float/exact/bool; everything else passes through, the **server stays
    the type authority**), and a two-pass contract:
    What a write may touch is read off the model, never guessed from the type text:
    `insert_columns` — the single authority `validate`/`row_iter`/`build_insert` all funnel through —
    drops any `ColumnInfo::is_server_assigned` column, so a file Schemaic exported can be imported
    back (a generated column matched by name used to pass validation and then fail the whole
    transaction); and `missing_required` warns on `!nullable && !auto_increment && default.is_none()
    && generated.is_none()` rather than on "integer primary key", which was silent on a natural
    `INT` key and noisy about every defaulted column. `validate` reads the whole file and returns
    *every* `Issue` with its line before anything is written, then `row_iter` streams coerced rows
    for the load. `build_insert` reuses `export::ident_sql`/`sql_literal`, so quoting can't drift
    from the SQL export. Note JSON columns are the union of every object's keys and the mapping is
    built from a *sample* — `trim_to_mapping` drops keys that first appear past it (CSV keeps the
    field-count check, where a mismatch means a stray delimiter). An array and JSON Lines read
    through one path: `ArrayUnwrap` blanks the wrapping brackets + top-level commas as bytes
    stream past, so a sample really stops at its limit instead of deserializing the file first
    (a whole-file walk still buffers, for the key union). NULL tokens match the *trimmed* field,
    except the empty one, which is exact — a blank field is data, and `trim` is the setting that
    says otherwise. `read_sample` is bounded at `SAMPLE_MAX_BYTES`, because a **record count is not
    a byte bound**: the CSV reader sets no field-size limit, so one stray `"` makes the whole
    remainder of a file a single unterminated field and a sample "of 200 records" reads to EOF —
    from a file the user only meant to look at. `target_verdict` over `DbNodeView` is the modal's
    other half: whether a schema change means the table it is open on has *gone*. `has_table` is an
    `Option<bool>` because "I looked and it wasn't there" and "I haven't looked" are the same
    `false` to a bool and a refresh empties what there is to look at, and the caller passes
    `same_connection` because `db_nodes` holds only the **active** connection's databases — nothing
    compared them, so switching connection discarded a hand-built mapping. Pure + unit-tested.
  - `ddl.rs` — **schema editing**, and every engine now reshapes a table, edits a view and edits a
    trigger. `supports_schema_editing(dialect)` is **gone**: it would have become "always true", and
    a vacuous predicate invites deletion for the wrong reason. The capabilities it used to answer
    for came apart into `supports_view_editing` and `supports_trigger_editing` — and both of those
    have since gone true everywhere too. They stay because the question is per *object* and the
    menus ask it per object, and each **computes** its answer out of `supports_change` instead of
    stating it: a predicate that discards its `SqlDialect` and returns `true` is the constant the
    *"ask a capability"* rule exists to prevent, wearing the name of the fix for it, and it answers
    for a fourth engine as confidently as for these three. Two more join them.
    `supports_table_design` is what the tree's **Edit table**, **Edit column** and **Edit index**
    entries ask — three literal `true`s until it existed — and its probe is a column retype, since
    that is the designer's own edit: expressible in place on MySQL and PostgreSQL, reached through
    the rebuild on SQLite, and either route counts. `supports_column_reorder` is the one where the
    engines genuinely disagree: MySQL places a column with `AFTER`, SQLite is created in the draft's
    order by the rebuild it is already doing, and PostgreSQL cannot move one at all. It is an
    exhaustive `match` rather than the `!= Postgres` it was spelled as at *both* its sites — in
    `diff`, which must not raise a move PostgreSQL has no statement for, and again on the designer's
    arrow buttons, which must not offer one. What actually varies for views moved down a level, into
    two narrower facts that are false on SQLite and only there. `supports_or_replace_view` — SQLite has no
    `CREATE OR REPLACE VIEW` in any form, so a redefinition there is a `DROP` plus a `CREATE`, the
    arm PostgreSQL already takes when `pg_replaceable` says no, reached unconditionally rather than
    on a body test. And `supports_view_rename` — SQLite has no verb that renames a view at all:
    `ALTER VIEW` is not a statement there, and `ALTER TABLE v RENAME TO v2` refuses with *"view v
    may not be altered"* (measured against the engine, not read off the grammar), so `diff_view`
    turns a bare rename into a re-create and the new name comes out of the `CREATE` half.
    `ChangeSet::view_statements`' `RenameView` arm is therefore spelled out per dialect with a
    `debug_assert!` on the SQLite one — unreachable by construction, and a `_ =>` there is exactly
    what would hand SQLite MySQL's `RENAME TABLE`. **Triggers needed the reader before the
    emitter**; that is `sqlite_trigger_info`, below. The schema tree's
    table/view menu, the editor's right-click and the Create submenu each ask the predicate for the
    object in front of them and offer **nothing** where the answer is no (absent, not dimmed — "not
    supported" rather than "not here").
    A third narrow fact joined them: **`requires_named_checks`**, false on SQLite alone —
    see `TableDraft::validate` below. And `supports_change` lists **`CreateTable`**, which
    it did not: `emit_sqlite` had no arm for it either, while `create_table_sql` has had a
    SQLite arm since the engine landed. A set holding one `CreateTable` is not empty, so
    the designer's New-table path opened its preview and offered Apply on an empty script
    — the plan reported success and no table existed. That shape is the reason both the
    capability list and the emitter's dispatch are worth reading together: a change kind
    missing from either is silent.
    **The twelve-step rebuild is `sqlite_rebuild_sql(current, draft)`**, over
    `Rebuild { current, draft }` — both sides, because which new column takes which old one's data
    can only be answered by looking at both. SQLite's `ALTER TABLE` does `RENAME TABLE`,
    `RENAME COLUMN`, `ADD COLUMN` and `DROP COLUMN` and nothing else, so every other edit is reached
    by creating the table the draft describes under a shadow name (`_schemaic_rebuild`, deliberately
    not the manual's `new_X`, since a collision with a real table fails the whole plan), copying the
    rows into it, dropping the original, renaming the shadow into place, recreating the indexes
    **after** the rename (an index name is unique per schema, so creating one earlier collides with
    the index it replaces) and replaying `TableInfo::dependent_ddl`. Each index goes back the way
    `ddl::sqlite_index_replay` says: emitted from the model, which is the only form that follows a
    renamed column into its index, or — for one the pragmas read `lossy` and this plan leaves alone
    — replayed verbatim from `IndexInfo::create_sql`. The copy is what makes a rename
    a rename rather than a drop-and-add: each new column is mapped to the one it came from, a column
    the user added is in neither list so its default applies, and a generated column is in neither
    because it cannot be inserted into. It is `INSERT … SELECT` and not `CREATE TABLE … AS SELECT`,
    which takes its column types from the query and discards every constraint. **A rename of the
    table itself is not part of it** — the rebuild always ends under the original name and
    `ALTER TABLE … RENAME TO` is emitted after it, natively, which is what makes SQLite repoint the
    references other objects hold. **And the plan carries `PRAGMA legacy_alter_table = ON` around
    the drop-and-rename**, or the rename fails on any view over the table: from 3.25 SQLite
    re-parses every view and trigger during `ALTER TABLE … RENAME`, and by then the original table
    is gone, so a view selecting from it kills the rebuild with `error in view v: no such table:
    main.t`. The pragma rides in the plan rather than in the backend because it is a property of
    these statements and not of the connection — and so the preview shows the whole procedure, which
    is the honest thing to put in front of someone about to approve it
    (`ddl::sqlite_rebuild_tests`).
    **The foreign-key guard rides in the plan for the same reason and one more.** The list opens
    with `ddl::FK_OFF` and closes with `ddl::FK_ON`, which is step 1 and step 12 of SQLite's own
    procedure: with enforcement on, the `DROP TABLE` is an implicit `DELETE FROM` the table and
    fires every `ON DELETE CASCADE` pointing at it — the table comes back exactly as drawn and
    another table has quietly lost every row. `sqlite::run_ddl` still sets the pragma out of band
    for its *own* execution, because SQLite ignores it inside a transaction; the copy in the plan is
    for the other consumer, since the same list is what the preview's **Copy** and **Open in
    editor** hand to a query tab, whose connection enforces foreign keys with nothing around it
    (`the_script_guards_itself_when_run_outside_run_ddl`). A table rename is inserted *before* the
    closing pragma so the whole procedure stays inside the guard.
    **What the rebuild writes is the model, so the model has to be the table.** Everything the
    declaration says and the pragmas don't report is read out of `sqlite_master.sql` through the
    shared boundary lexer — each column's `COLLATE` (`sqlite::collations_of`), the `AUTOINCREMENT`
    keyword (`declares_autoincrement`, not `sqlite_sequence`, which has no row until the first
    insert) — and `WITHOUT ROWID`/`STRICT` come from the `pragma_table_list` row `has_rowid`
    already reads. A `UNIQUE` constraint's `sqlite_autoindex_*` is **not** an index statement: it is
    re-declared as a `UNIQUE (…)` line inside the rebuilt body, from the *draft's* column names, so
    it follows a rename — `sqlite_index_replay` answers `Skip` for it, since the engine refuses
    `CREATE UNIQUE INDEX "sqlite_autoindex_u_1"` by name. And the draft's `CHECK` predicates are
    re-pointed across a rename here, because here is where the declaration is written; the
    MySQL-family repair `ddl::alter_column_disturbs_checks` gates is a different repair for a
    different statement. `db::sqlite::rebuild_fidelity_tests` reads `sqlite_master.sql` back after
    each of these and compares — the question the consequence-shaped suites beside it cannot ask.
    **`AUTOINCREMENT`'s counter is carried too**, and it is not part of the model: it is a row in
    `sqlite_sequence` that `DROP TABLE` takes with it, while the copy re-seeds the new table's from
    the rows that *survived* — so a table whose highest rows had been deleted came back handing
    those ids out a second time, which is the one thing the keyword promises never happens. Two
    statements between the copy and the drop move the row onto the shadow table (a `DELETE` first:
    `sqlite_sequence` has no unique index, so `INSERT OR REPLACE` would leave two rows for one
    table), and the rename carries it back under the real name. Gated on **both** sides declaring
    the keyword — `declares_sqlite_autoincrement`, over `sqlite_inline_key`, which is also what
    `create_table_sql` asks, so there is one definition of SQLite's inline-key rule.
    **Three things the rebuild refuses rather than does.** `ChangeSet::unsupported` withholds a plan
    whose replayed `dependent_ddl` would name a column the plan renames or drops
    (`rebuild_strands_a_trigger`): the text is a snapshot, `legacy_alter_table = ON` stops SQLite
    fixing it, and SQLite validates `NEW.<col>` at write time rather than at `CREATE TRIGGER` — so
    the plan used to *succeed* and the table then rejected every write. The route that does work is
    offered instead: a rename **on its own** is `ALTER TABLE … RENAME COLUMN`
    (`supports_change` + `is_rename_only`), which re-points every view and trigger for us.
    It also withholds a rebuild of a table whose *declaration* says more than the model can restate
    (`rebuild_cannot_restate`, over `unrestatable_sqlite_clauses`): a foreign key's `DEFERRABLE`, a
    column's `ON CONFLICT`, a `DESC` primary-key column. What introspection doesn't model, the
    rebuild deletes — and because the draft is built from the same incomplete model, `diff` reads
    the untouched draft as a no-op, so no round-trip check can see the loss either. Each was
    measured deleted by a plan that reported success, and each changes what the table *does*. The
    scan is the shared boundary lexer's and covers the table body only, so an **index** key's `DESC`
    — which the model does carry (`IndexColumn::descending`) and does re-emit — is not a refusal.
    `Change::RebuildTable(Box<Rebuild>)` is how it reaches a plan: `diff` inserts one at the
    **front** of the set the moment that set holds a change SQLite has no statement of its own for,
    and that one change performs the whole set. It sits *beside* the changes it performs rather than
    instead of them, so the preview still lists the user's edits in their own terms and one line
    says how they will happen. The trigger is "is there a change here with no statement of its own",
    not "is this an alter" — a set of nothing but the drops the engine does have keeps its direct
    path and pays nothing. **An `ADD COLUMN` is the one fast path taken**, through
    `sqlite_native_add(column, position)`: appending a column is the commonest designer edit there
    is and SQLite performs it instantly, so copying the whole table to achieve it was correct and
    absurd. The predicate answers false for anything it isn't sure of, because the failure mode is a
    plan that *half-applies* — fast path taken, engine refuses the statement, and the edit the
    preview promised is simply gone. A needless rebuild is slow; a wrong fast path is a lie. The
    rules, each measured against SQLite 3.46 rather than read off the grammar: `position` must be
    `None`, since `ADD COLUMN` always appends and a column dropped into the middle would land at the
    end, leaving the designer showing one order and the table having another — **this is the one
    rule with no error message behind it, the statement succeeds, in the wrong place** (and
    `apply_positions` runs for MySQL and SQLite alike, so a `Position` is a reliable signal); no
    primary key (*"Cannot add a PRIMARY KEY column"*); no `auto_increment`, because `AUTOINCREMENT`
    is legal only inline as `INTEGER PRIMARY KEY AUTOINCREMENT` and `ColumnInfo::definition_sql`
    therefore drops it for SQLite, so a native add would silently lose the counter the rebuild's
    table builder can place; a constant default if there is one (*"Cannot add a column with
    non-constant default"*); and `NOT NULL` requires a non-null default (*"Cannot add a NOT NULL
    column with default value NULL"*). Two deliberate non-rules: **uniqueness** isn't on
    `ColumnInfo` at all — it arrives as an index, which has no native arm and takes the set back to
    a rebuild by itself, and that is a fact about *this gate* rather than about SQLite, which
    refuses only an inline `UNIQUE` in the column definition and would take a native add followed by
    a `CREATE UNIQUE INDEX` quite happily — and a **generated** column *is* addable, since the
    emitter writes no `VIRTUAL`/`STORED` keyword so SQLite's own default (`VIRTUAL`) applies and
    `STORED` is the form the engine refuses; it carries its expression instead of a default, so the
    null-default rule has nothing to reach. `sqlite_constant_default` decides the default: the `CURRENT_TIME`/
    `CURRENT_DATE`/`CURRENT_TIMESTAMP` keywords and anything parenthesised are not constants (the
    paren test also catches a bare `now()`, which isn't a legal `DEFAULT` there at all), and the
    parenthesis is looked for at a **code** position through `sql::skip_noncode` rather than with a
    `contains('(')`, so a default of `'a (b)'` stays native — its parens are data. A native add
    *beside* an unsupported change is still subsumed by the rebuild and must not also emit its own
    `ADD COLUMN`. There is a test per restriction in `ddl::sqlite_designer_tests`, and
    `db::sqlite`'s `every_natively_added_column_is_one_sqlite_accepts` runs every shape the
    predicate calls native through `run_ddl` at real in-memory SQLite — which is what the fast path
    actually rests on, since a predicate drifting from the engine's restrictions can't be caught by
    reasoning, only by asking SQLite.
    **`supports_change(dialect, &Change)` is the second gate, and it answers a different
    question.** It is about one change **on its own** — a context-menu shortcut, which has no draft
    behind it to build a table from — where the designer's plan can answer anything by rebuilding.
    On SQLite it is true for `DropTable`, `DropView { materialized: false }`, `DropColumn`,
    `DropIndex { constraint: None }` and `DropTrigger` — the drops the engine genuinely has
    statements for — for the whole-statement objects it creates like anyone else (`CreateView`,
    `ReplaceView`, `CreateTrigger`, `ReplaceTrigger`, the replaces being a drop-and-create on every
    engine), and for `RebuildTable`, which only `diff` raises. `RenameView` is deliberately **not**
    on the list: `diff_view` resolves a SQLite rename into the re-create before it can reach here.
    **It is no longer purely a question of the change's kind**: `AddColumn` is answered by asking
    `sqlite_native_add` above, the one arm that depends on what the change *contains*, and it falls
    through to that list only after. It is false for everything else, where each false is
    the twelve-step rebuild in disguise: a foreign key or a constraint-backed index comes off only
    by recreating the table around it. Every non-SQLite dialect answers true. It exists because the
    per-row menus were built with **no** gate at all — not this one, not even `read_only` — so a
    column row's **Edit column** and a key row's edit entry opened the designer on a SQLite
    connection, ran `diff`, and reached a preview that only `Db::run_ddl` refused at the last
    moment. Those two rows now ask `overlays::field_entries`/`key_entries` (separate from the menu
    builder so the rule can be asserted without a `Ui`, the same shape as `object_entries`): Edit
    column and the key row's edit entry — **Edit index**, **Edit foreign key** or
    **Edit primary key**, named for whichever the row is — are offered on every engine, since the
    designer is the thing that reaches a rebuild, **Drop** (column) and **Drop index** stay because
    the engine performs them, and Drop foreign key — plus Drop index when the index backs a
    constraint — is absent (`overlays::row_menu_tests`). `ChangeSet::unsupported()` is the same
    predicate read over a whole set, returning the plain-language summaries of what the dialect
    can't express, which is what the preview shows and refuses to apply around. **Where a rebuild is
    present it withholds nothing except an index flagged `IndexInfo::lossy` that cannot be put back
    as it was**: the rebuild drops the table, so every index has to be created again, and one
    re-emitted from a partial reading is not the index that was there — a partial index comes back
    covering every row, an expression index missing its key. The way through is the one a trigger
    takes: `ddl::sqlite_index_replay` replays the index's **own** `CREATE` text
    (`IndexInfo::create_sql`) verbatim, which cannot lose what the pragmas never read. That leaves
    three narrow cases where no faithful statement exists, and only those are withheld — the index
    was **edited** (the stored text is the old one, and the model can't describe the new one), the
    index has no stored text of its own, or the plan **renames or drops a column**
    (`ddl::column_moved`). The last is not caution: the unread part is exactly where a partial
    index's predicate and an expression key live, so which columns the index uses is a question the
    model cannot answer, and it is asked of the whole table instead. A withheld index is not a
    warning to read past; the plan is refused until the user drops it or undoes the edit.
    `TableDraft` (the desired table; column/index/FK
    entries each carry the name they had on the server, which is what tells a *rename*
    from a drop-plus-add) → `diff(current, draft, dialect) -> ChangeSet` → `emit()`.
    Every `Change` answers `summary()` and `risks()`, which is what the preview
    modal renders — `risks()` returns *every* consequence, not the first, because one edit can
    narrow a column **and** make it NOT NULL and the NOT-NULL sentence ("the statement fails")
    otherwise reads as a promise that nothing is lost. The emitter owns the engine divergence: MySQL coalesces into one
    `ALTER TABLE` and restates a whole column via `definition_sql` (`MODIFY` replaces it,
    so anything omitted is destroyed); PostgreSQL splits renames / `DROP INDEX` /
    `CREATE INDEX` / `COMMENT ON` into their own statements and drops a key by
    *constraint* name (`IndexInfo::constraint` — it has no `DROP PRIMARY KEY`).
    **SQLite has its own arm (`emit_sqlite`) rather than the fall-through to MySQL's it used to
    take**, because two shapes there are refusals and not merely infelicities: its `ALTER TABLE`
    takes exactly one operation — there is no clause list — so two dropped columns are two
    statements, and an index comes off by a standalone `DROP INDEX` as on PostgreSQL, MySQL's
    `ALTER TABLE … DROP INDEX` having no SQLite form at all. It emits indexes before columns
    (SQLite refuses to drop a column an index still names, so the two in one plan only work in
    that order) and filters every change through `supports_change`, so a gate that drifts open
    shows an empty preview instead of handing SQLite MySQL's spelling of the change
    (`ddl::sqlite_drop_tests`). Its `AddColumn` arm writes
    `ALTER TABLE <t> ADD COLUMN <definition>` from `ColumnInfo::definition_sql` — the same column
    emitter the rebuild's `CREATE TABLE` uses, so the two can't disagree about what the column is,
    only about how it gets there — and adds come **after** drops, so a column replacing a dropped
    one of the same name finds the name free. Where the set holds a `RebuildTable` that arm returns
    the rebuild's statements and the trailing `RENAME TO` **and nothing else** — the other entries
    describe what the rebuild achieves, so emitting them beside it would apply the same edit twice.
    `create_table_sql` splits on the same fault line, and for the same reason: it used to split on
    PostgreSQL and give everything else MySQL's shape, which for SQLite is not unidiomatic but
    invalid — inline `KEY`/`UNIQUE KEY`, `ENGINE=`, `COLLATE=` and `COMMENT=` are each a syntax
    error there. It now sides with PostgreSQL on indexes (statements of their own) and has no table
    options at all — except the two suffixes that change what the table *is*, `WITHOUT ROWID` and
    `STRICT`, which the draft carries and a rebuild that didn't restate them silently dropped. Its
    one real divergence is the **inline single-column key**, which is legal only as
    `INTEGER PRIMARY KEY [AUTOINCREMENT]`: that column takes the whole declaration and the
    table-level `PRIMARY KEY (…)` clause stands down rather than declaring a second key (a composite
    key is unaffected). **`AUTOINCREMENT` is a narrower question than "server-assigned"** and has
    its own flag, `ColumnInfo::sqlite_autoincrement`: every `INTEGER PRIMARY KEY` is the rowid and
    is filled in for you, which is what `auto_increment` says, while the keyword adds the promise
    never to reuse an id and a `sqlite_sequence` row to keep it — so reading the first as the second
    put `AUTOINCREMENT` on every plain key a rebuild touched. `ColumnInfo::definition_sql` makes the
    same distinctions — SQLite keeps `COLLATE`, the generated expression (with `STORED` when
    `generated_stored`, since its default is `VIRTUAL`), `NOT NULL` and `DEFAULT`, parenthesising an
    expression default because `pragma_table_xinfo` strips the parentheses the grammar requires
    (`schema::is_bare_sqlite_default`), and drops `AUTO_INCREMENT`, `ON UPDATE` and the column
    comment (`ddl::sqlite_create_tests`). Ordering
    is dependency-first (FKs and indexes off before the columns under them; keys back on
    after). `normalize_type`/`types_equal` + `defaults_equal` are the reason a designer
    opens clean — `int(11)` ≡ `int`, `character varying(45)` ≡ `varchar(45)`. **The
    round-trip gate is test-enforced**: `TableDraft::from_table(t)` diffed against `t`
    must be empty over captured fixtures from classicmodels/sakila/employees/world +
    PG world/chinook + two SQLite shapes (`ddl::tests::roundtrip`) — extend those
    fixtures rather than working around them, since any model-fidelity gap surfaces to
    the user as a phantom change. **On SQLite it is worse than a stray preview line**:
    a non-empty diff is what routes an edit through the twelve-step rebuild, so an
    invented change drops and recreates the table for an edit nobody made. The gate has
    a second half a fixture cannot cover — `db::sqlite`'s
    `an_introspected_table_diffs_to_nothing_against_its_own_draft` makes the same
    assertion over **real introspection**, across a corpus declaring every fidelity
    property the engine added; a fixture states what the reader is believed to produce,
    and that belief is exactly what the range's findings kept disagreeing with. It found
    three shipped bugs on its first run (`CreateTable` unhandled by `emit_sqlite`,
    `validate` refusing an unnamed `CHECK`, and two unnamed checks pairing onto one
    original). Also `key_list_text`/`parse_key_list` (the designer's `bio(20), age DESC`
    field) and `common_types`. Pure + unit-tested.
    **A `CHECK` is matched to its original by name, and an unnamed one has none.**
    SQLite keeps a constraint unnamed — a bare `CHECK (a > 0)` in the table body is the
    ordinary spelling there and `CheckInfo::clause_sql` emits it back that way — so
    `diff` claims by name first and then pairs the unnamed ones **by predicate**, each
    original claimable once, with whatever is left on either side the real drop or add.
    Matching on the shared empty name gave every unnamed draft check the *first*
    original and produced a `DROP`+`ADD` on an untouched table.
    **`TableDraft::validate(dialect)`** takes the dialect for one rule:
    `ddl::requires_named_checks` is false on SQLite alone, because MySQL and PostgreSQL
    assign a constraint name themselves and a blank one there can only be an unfinished
    form. Demanding one everywhere made every SQLite table carrying an unnamed check
    **uneditable** — the designer opened on an untouched table with a blocking footer
    error and no field to fix it in. The predicate rule (`CHECK ()`) is unchanged and
    applies to all three.
    **`TableDraft::find_key(index, foreign_key) -> Option<DraftKey>`** says where one of a table's
    keys sits in the draft — which of the designer's sections holds it, and which row of that
    section — and it exists because the schema tree's sequence of keys and the draft's are **not
    the same sequence**, so a position taken from one lands on the wrong row in the other. Three
    ways they differ, one per `DraftKey` arm. The tree lists the primary key among the keys while
    `TableDraft::from_table` filters `is_primary()` out of `indexes` entirely, so an index position
    read off `TableInfo::indexes` is one row late in any table that has a primary key
    (`DraftKey::Index`). The tree shows a foreign key under its **backing index**, whose name
    needn't match the constraint's — classicmodels' `customerNumber` index backs `orders_ibfk_1` —
    while the draft keeps `foreign_keys` as a collection of its own, so the lookup is by
    *constraint* name and the index name is ignored (`DraftKey::ForeignKey`). And the primary key
    is no row anywhere in the draft: it is the per-column tick `set_in_primary_key` writes, which
    is what the index form's own hint — *"No index selected. The primary key lives on the
    columns."* — tells the user, so `DraftKey::PrimaryKeyColumn` names its **first column in key
    order** instead. The arguments are exactly what a `CtxKind::Key` tree row carries, asked
    together because they answer one question. `None` for a key this draft doesn't hold, which is
    the caller's cue to open on nothing in particular rather than on row 0
    (`find_key_positions_an_index_in_the_primary_key_less_list`,
    `find_key_sends_the_primary_key_to_its_first_column`,
    `find_key_resolves_a_foreign_key_by_its_constraint_name`,
    `find_key_answers_nothing_for_a_key_the_draft_does_not_hold`).
    **CHECK constraints** are `CheckDraft`/`CheckInfo` → `Change::{AddCheck, DropCheck}`,
    a drop-and-add on both engines (neither can alter one in place). Three rules are
    written down because each was a bug waiting: the predicate is normalized **on the
    way in** by `check_predicate` (PG's `pg_get_constraintdef` returns the whole clause,
    MySQL's `CHECK_CLAUSE` a parenthesised predicate, a person types neither), so the
    model holds it bare and the emitter wraps it exactly once; `checks_equal` compares
    modulo wrapping parens and whitespace runs — via `peel_parens`, which goes through
    `skip_noncode`, since `name <> ')'` has a close-paren in a string — but deliberately
    *not* modulo tokenisation, so `qty>0` vs `qty > 0` costs a re-validating drop-and-add
    rather than risking a kept edit; and a column rename is rewritten into the
    predicate **only where the server won't do it itself** — PostgreSQL rewrites its own
    stored parse tree and MariaDB rewrites a table-level check's text, while MySQL 8
    *refuses the rename outright* (`ERROR 3959`) unless the constraint comes off first,
    so there and only there `diff` adds a `DropCheck`+`AddCheck` pair with the predicate
    re-pointed through `repoint_check_column` (a **token walk** over `skip_noncode`, so
    `qty` never matches inside `qty_total`, inside `'qty'`, or in a comment).
    `DropCheck` carries a risk sentence though it deletes no data — the table stops
    guaranteeing something and nothing else says so — but `ChangeSet::destructive`
    suppresses it when the same name is re-added in the same plan, since every check
    *edit* is a drop-and-add and "rows the constraint refused are accepted from now on"
    is simply false about one. `enforced` is MySQL's `NOT ENFORCED` only: PG's
    `NOT VALID` exempts existing rows and so can't silently change what a write does.
    **`CheckInfo::column_level` is MariaDB's `CHECK_CONSTRAINTS.LEVEL`**, and it is the
    one place a check is not a table-level object: `q INT CHECK (q > 0)` makes a
    constraint that is *part of the column*, so `MODIFY`/`CHANGE COLUMN` deletes it
    unless the clause restates it (measured on 10.11.14 — the constraint is simply gone
    and the next `-5` is accepted). It has no name of its own — the syntax refuses a
    `CONSTRAINT` label at column level, MariaDB names it after its column and renames it
    with the column — and `DROP CONSTRAINT` cannot address one at all (`ERROR 1091`).
    So the emitter goes through the column both ways: `Change::AlterColumn::inline_check`
    restates an unchanged one (re-pointed when the column is renamed, or the `CHANGE` is
    `ERROR 1054`), and a check the draft **dropped or edited** has its impossible
    `DropCheck` swapped for a `MODIFY COLUMN` without the clause. MySQL 8 has none of
    this — it folds the same syntax into a table constraint at `CREATE` time — so all of
    it is gated on `ServerFlavour::MariaDb`, and a MariaDB older than 10.5 (no `LEVEL`
    column) degrades to the pre-flag behaviour rather than losing its checks.
    **Views** ride the same rails: `ViewDraft` (name + body + the `ViewOptions` it
    carries) → `diff_view` → `Change::{CreateView, ReplaceView, RenameView, DropView}`
    → the same preview. One engine rule each lives here. MySQL's `CREATE OR REPLACE VIEW`
    replaces the *whole* view, so the emitter restates `ALGORITHM`/`DEFINER`/
    `SQL SECURITY`/`CHECK OPTION` — omitting the security type silently turns a
    `DEFINER` view into an `INVOKER` one, which is a privilege change, the same class
    of bug as `MODIFY COLUMN`'s. PostgreSQL's may only **append** columns, so an edit
    that renames/retypes/reorders one needs `DROP` + `CREATE`, which takes dependent
    views and grants with it: `pg_replaceable` (over `intel::select_output_names`)
    decides where it can, **uncertainty resolves to replace-and-let-the-server-refuse,
    never to drop**, and `ViewDraft::force_recreate` is the user's override.
    SQLite's is the pair of predicates above: every edit is a drop and a create, so nothing is
    carried through a *replace* there — it is carried through the re-create, which is why
    `ViewOptions::column_list` had to be modelled (see `core::schema`). `create_view_sql`
    asks per engine (`my`/`pg` locals) rather than `!pg`, which had been sorting SQLite onto
    MySQL's side and would have emitted `ALGORITHM`/`DEFINER`/`SQL SECURITY` at an engine that
    has none of them; the check option is likewise MySQL-and-PostgreSQL only. `emit_sqlite`
    now calls `view_statements`/`trigger_statements` rather than keeping a hand-rolled
    `DropView` arm, so there is still one view emitter.
    The MySQL `ALGORITHM` a replace would reset arrives *after* the editor opens
    (`SchemaActions::view_algorithm` → `Db::view_algorithm`; see `schemaic-db`), and
    `view_editor::fetch_algorithm` patches **both** sides of the diff with it — writing
    only the draft would make every MySQL 8 view open already-changed against a `current`
    that still said `None`. It leaves the draft alone if the user already picked one. Materialized
    views are drop-only (no `CREATE OR REPLACE` exists for one). Same round-trip gate,
    same rule about extending fixtures.
    **PostgreSQL's standalone objects** — enums, domains, sequences — are `EnumDraft`/
    `DomainDraft`/`SequenceDraft` → `diff_enum`/`diff_domain`/`diff_sequence` → the same
    preview, with `ObjectKind` folding the three identical-but-for-a-keyword changes
    (`RenameObject`/`DropObject`/`SetObjectComment`) into one arm each. The shape is dictated
    by what PostgreSQL *can't* do. `enum_value_plan` is the heart of it: appending, inserting
    and renaming a value are `ALTER TYPE`, but there is no `DROP VALUE` and no way to move
    one, so the moment a value is **removed or reordered** the whole edit collapses into a
    single `Change::RecreateEnum` — `recreate_type_sql`'s park-create-recast-drop dance, which
    casts each dependent column through `text` — **and for an enum only, on to the rebuilt
    type**, because `text` has no assignment cast to one. A *domain* stops at `::text`
    deliberately: the second, explicit cast is what silently truncates (`varchar(64)` →
    `varchar(16)` destroyed 48 characters per row and committed), where the assignment cast
    PostgreSQL then applies refuses instead. `RecreateDomain` therefore carries the base type
    it had *before* the edit, so `risks()` can run the same `column_risks` narrowing analysis a
    column's type change gets — it was the one narrowing path in the emitter that disclosed
    nothing. It also restates the default it had to drop first. A domain's
    base type is the same story (`ALTER DOMAIN` has no action for it); everything else about
    one alters in place. `type_dependents` reads the columns that dance has to touch off the
    introspected schema, matched on the **type's** identity and scanned across *every*
    namespace (a qualified declaration must match both halves; an unqualified one is the
    default namespace's type wherever the table lives, which is what `format_type` means by
    printing it bare). It is deliberately a **lower bound** — a view or function built on
    the type can't be enumerated from `DbSchema` at all — which is why `recreate_risk` names
    the columns *and* says the server may still refuse. Two disclosures exist because the
    model can't distinguish the intent: a value list can't tell a rename from a
    delete-plus-add, so the plan takes the reading that keeps the data and
    `RenameEnumValue::risks` says every row will be relabelled; and `AddEnumValue` warns that
    PostgreSQL can never take a value back. A sequence's `restart` is on the **draft, not the
    model** — it's an action, not a state, so folding it in would make every re-opened editor
    dirty against a sequence nothing had changed — and `sequence_edits`/
    `sequence_alter_clauses` are kept in step so the sentence the user reads and the statement
    that runs come from one comparison. All PostgreSQL-only; `object_statements` is still
    called from both emitters so a MySQL connection handed such a set emits SQL the server can
    reject rather than dropping it on the floor.
    **Triggers and stored routines** ride the same rails again:
    `TriggerSetDraft`/`TriggerDraft` → `diff_triggers` and `RoutineDraft` → `diff_routine`
    → `Change::{CreateTrigger, ReplaceTrigger, DropTrigger, CreateRoutine, ReplaceRoutine,
    RenameRoutine, DropRoutine}` → the same preview. None of the three can *alter* a trigger,
    so **every** edit is a drop-and-create and `ReplaceTrigger` is that pair — which is why
    `trigger_statements` emits **all the drops, then all the creates** rather than each pair
    together: adjacent pairs collide the moment two triggers swap names, and on MySQL
    statement 1 has already committed when statement 2 fails, so the first trigger is simply
    gone. Same rule, same reason as `GridWrite::plan` in `core::model`.
    `session_wrapped_with` is the MySQL half of that emitter, shared by triggers
    (`session_wrapped_create`) and routines (`session_wrapped`): neither `CREATE TRIGGER` nor
    `CREATE PROCEDURE` has a clause for the `sql_mode`/`character_set_client`/
    `collation_connection` the object was written under, yet all three are part of what it does,
    so the values are set on the session around the statement and restored after (`run_ddl` runs
    a MySQL plan in order on one connection, which is what makes that safe). **The routine half is
    the later of the two and closes a hole of its own**: every MySQL edit to a routine is a `DROP`
    that commits on its own followed by a `CREATE`, so an unwrapped recreate re-filed the routine
    under whatever `sql_mode` the applying session happened to have — one written under
    `sql_mode = ''` and recreated under a strict one starts raising on rows it used to truncate,
    with the original already gone. One helper rather than two because it is the same failure, and
    the wrap goes on **every** `CREATE` the routine emitter produces, not only a recreate's.
    Nothing is emitted
    when nothing is known — `None` means "not fetched", and inventing a session state is a change
    nobody asked for. It early-returns on `!= MySql`; the old `== Postgres` test would have handed
    SQLite `SET SESSION sql_mode = …`.
    **A routine's diff is per-engine at the *rename*, and that is the whole shape of
    `diff_routine`.** PostgreSQL replaces one in place (`supports_or_replace_routine`) and renames
    it with a statement of its own (`supports_routine_rename`), so a rename is a separate change
    ordered *after* the redefinition — which has to address the signature the server still holds.
    MySQL has neither verb: every edit there is a `DROP … IF EXISTS` plus a `CREATE`, so a rename
    is folded into that recreate (the drop names the old routine, the create the new one) and a
    bare rename reads as a redefinition, exactly as `diff_view` resolves one on SQLite. Which
    route a change took rides on `Change::ReplaceRoutine { recreate }` rather than being re-derived
    at emit time, so the preview's risk sentence and the SQL cannot disagree — and `recreate` is
    what earns the "a definition the server rejects leaves no procedure at all" warning, the same
    sentence `ReplaceTrigger` carries and for the same reason. `RoutineInfo::signature_sql` is what
    knows the two engines address a routine differently: PostgreSQL by its **argument types**
    (overloads share a name), MySQL by name alone (the parameter list there is a syntax error on a
    `DROP`). The list it writes is `RoutineInfo::identity_arguments` —
    `pg_get_function_identity_arguments`, fetched alongside `pg_get_function_arguments` and empty on
    MySQL, whose routine identity is the bare name. A second field rather than a reformatting,
    because the two strings genuinely differ and each is a syntax error where the other belongs:
    `CREATE` needs the defaults (`b boolean DEFAULT false`), while `DROP`/`ALTER … RENAME` take
    `[argmode] [argname] argtype` and answer `syntax error at or near "DEFAULT"` to anything more —
    so while the `CREATE` form was spliced in here, a routine with a defaulted parameter could not be
    dropped or renamed from the app at all. It falls back to `arguments` for a routine assembled by
    hand rather than read from a catalogue, where the two are the same string.
    `Change::ReplaceRoutine` therefore carries the routine **as the server holds it**
    (`server: Box<RoutineInfo>`) beside the draft: a recreate's `DROP` must name that one and not
    `draft.info`, which differs in the *name* on MySQL (where a rename rides the recreate) and in the
    *parameter list* on PostgreSQL (where an edited list is a different function, so the `DROP`
    dropped nothing and the `CREATE` left the original standing beside a new overload). It is what
    `summary` names as the thing being changed, and what `risks` compares against to warn that
    callers passing the old arguments no longer resolve.
    That is also why `ObjectKind::Function`/`Procedure` are on the browse enum but refused
    by `drop_object`, which has only a name to work with — it returns an **empty** change set for
    one, so a preview says "no changes" rather than a bare-name statement that is right up until
    the first overload; `drop_routine` takes the whole `RoutineInfo` and is the route.
    SQLite has no stored routines at all — a function there is registered by the host program,
    not stored in the database — so its arms are absent from `supports_change`,
    `supports_routine_editing` is false, and the tree grows no folder and the Create menu no entry.
    **SQLite got here through the reader, not the emitter.** It publishes no catalogue of a
    trigger's parts — no `information_schema`, no pragma, only the statement text — so
    `sqlite_trigger_info(create_sql)` is what made an editor possible at all, and until it existed
    the list was empty and an editor over it would show a table's triggers as gone and offer to
    "add" one that is already there. **Structure from the AST, body from the text**: the
    per-dialect sqlparser AST (`intel`'s `SqlDialect::parser()`) answers the timing, event,
    `UPDATE OF` columns and `WHEN` guard, which a scanner gets wrong on a body containing the same
    words, while `sqlite_trigger_body` takes the `BEGIN … END` block verbatim over `skip_noncode` —
    re-printing it from the AST would normalise away the user's comments, casing and line breaks,
    which is a phantom change on every open and a rewritten trigger on every apply. A statement it
    can't read yields `None` and the caller drops it, safe in the one way that matters because
    `diff_triggers` only drops what the *server copy* lists. `TriggerDraft::validate`'s SQLite arm
    is the engine's own grammar, every rule measured against 3.45 rather than inferred: one event
    per trigger, no `TRUNCATE`, `INSTEAD OF` only on a view *and* a view taking only `INSTEAD OF`
    (`cannot create INSTEAD OF trigger on table` / `cannot create BEFORE trigger on view`), row
    level only (`FOR EACH STATEMENT` is a syntax error), and a body that must be a `BEGIN … END`
    block with at least one statement in it — a bare statement and an empty block are both syntax
    errors. `is_begin_end_block` asks that last one through the shared lexer rather than with a
    `starts_with`, since a body may open with a comment and a `BEGIN` inside a string is not the
    block's own. It is refused in the modal rather than at Apply because the `DROP` has already run
    by the time the `CREATE` fails.
  - `erd.rs` — the **ER-diagram** model (the UI half is `ui/erd_view.rs`). `build_graph` turns an
    introspected `DbSchema` into a `DiagramGraph` — nodes = tables, edges = FKs — seeded either by
    `DiagramSeed::Database` (whole database, hiding FK-less "island" tables) or `::Table` (one
    table's one-hop neighbourhood). A cross-database FK target can't be enumerated from one
    `DbSchema`, so it becomes an unexpandable `NodeKind::Stub` rather than a missing edge.
    `should_collapse`/`collapsed_visible` decide per-card density (a wide or crowded table collapses
    to a pinned PK/FK subset) and `column_row_offset` is what still anchors an FK edge to the right
    row on a collapsed card. `layout`/`place` are a deterministic layered auto-layout (nodes layered
    by longest FK-dependency chain, ordered by neighbour barycentre), so the same schema always
    arranges the same way; `edge_anchors`/`cubic_controls`/`sample_cubic`/`nearest_polyline` are the
    pure bezier geometry + hover hit-test the custom paint canvas uses. `DiagramLayoutsFile`
    persists manual drags per `(conn_id, database)` to `diagrams.json`, falling back to auto-layout
    for an unknown or stale id.
    **Find-in-diagram** is the pure half of the modal's Ctrl+F bar. `search(graph, needle)` returns
    one `NodeMatch { node, name, columns }` per node the term touches, kept per-card because both
    things the diagram does with a search are per-card: highlight the matched parts of a card, and
    pan to it when every hit landed in one. The term is trimmed and lower-cased here so the UI stays
    a thin caller, and every name goes through `schema::object_name_matches` rather than a third
    hand-rolled `to_lowercase().contains` (see the one-predicate invariant); an empty or
    whitespace-only term finds nothing, which is that predicate's rule. A `NodeKind::Stub` answers
    on its name alone, having no columns to search. `hits()` counts the name as one plus one per
    matched column, `total_hits`/`match_label` are the bar's readout, and `sole_node` — the
    pan-and-flash trigger — asks about **cards, not hits**: three matches inside `orders` still name
    one place to go, while two cards leave the choice to the user and moving the canvas would only
    be guessing which was meant. The stated limitation is that a collapsed card shows only its key
    columns, so a matched column can be real and off-screen — the count is the truth about the
    diagram and the card outline is what says where to look. Auto-expanding the card to reveal it is
    deliberately not done: that resizes the card and re-routes every edge touching it, as a side
    effect of typing. `search_tests` covers case and surrounding space, the empty needle, a stub,
    hits versus cards, and each form of the readout.
  - `erd_export.rs` — the ER diagram's **export** renderers, one per format, all pure (the UI half
    is `ui/erd_view.rs`'s scene builder + `ui/erd_raster.rs`). Two families, answering different
    questions. **Text** — `to_mermaid`, `to_dbml`, `to_plantuml`, `to_dot` — emits the *graph*:
    every node with all its columns and every FK with its cardinality, ignoring positions and
    collapse state, because those are properties of this app's canvas and mean nothing to the tool
    on the other end. **Picture** — `to_svg` — renders the *arrangement*, from an `SvgScene` the UI
    fills from its live signals rather than re-deriving any geometry, so the file cannot drift from
    what is on screen; PNG is that same document rasterised, with no second renderer anywhere.
    The formats that need a bare identifier (Mermaid entities, PlantUML aliases, Graphviz ports)
    go through `aliases`, which is **collision-broken on purpose**: `sales.orders` and
    `sales_orders` both slug to `sales_orders`, and a Mermaid file naming two entities the same
    silently merges them into one card with interleaved columns and no error anywhere. DBML and
    Graphviz keep the real names, quoted or escaped. `crow_ends` is the shared cardinality notation
    and reads the *parent* end from `DiagramEdge::optional` — a nullable FK means a child may
    reference nothing, so "exactly one" drops to "zero or one" — while the child end is "zero or
    more" (`o{`) or "zero or one" (`o|`) from the uniqueness `core::erd` already worked out: zero
    either way, since nothing obliges a parent row to have children. **Three of the four text
    exports say that the same way.** Mermaid and PlantUML take the two strings as they are;
    Graphviz cannot, so
    `to_dot` spells the child end as the composite `crowodot`/`teeodot` — the `o` *modifier* applies
    only to Graphviz's fillable primitives, so `ocrow` parses and then draws exactly like `crow`,
    and in a composite the later shape sits farther from the node, which is where crow's-foot puts
    the zero. Without it the `.dot`'s tail read "exactly one" and asserted every parent row has a
    child, which the `.mmd` and `.puml` of the same diagram explicitly deny. **DBML is the fourth
    and says none of it**: its grammar has no optionality notation at all, so
    `Ref: orders.user_id > users.id` is the whole vocabulary and `to_dbml` never calls `crow_ends`
    or reads `DiagramEdge::optional` — it matches on the cardinality alone, writing `>` for
    many-to-one and `-` for one-to-one, and a nullable FK produces the same line as a `NOT NULL`
    one. Nothing is lost: the nullability survives as the column's own `[not null]`, which is where
    a DBML reader looks for it. The **canvas** is the
    one surface that draws no zero at the child end, and deliberately: on screen that marker would
    be on every edge of every diagram without exception, so it separates nothing and costs twenty
    more stroked segments per edge on the app's heaviest paint — `crow_ends` and `erd_view`'s
    `marker_lines` each carry that argument in their rustdoc, so read both before changing either.
    `mermaid_type`
    exists because a space ends Mermaid's type token: MySQL's `int unsigned` and PostgreSQL's
    `timestamp with time zone` would otherwise turn the rest of the line into a parse error.
    `to_dot` deliberately emits **no `pos` attributes**: they bind only under `neato -n`, so a file
    claiming to carry the user's layout would silently ignore it under plain `dot`.
    On the picture side, `n()` writes coordinates at two decimals — not cosmetic, since a bezier
    sample formats as `104.30000000000001`, an edge polyline carries 34 of them, and the noise is
    both most of the file and what makes two exports of one layout undiffable. Per-card colours
    (`header_fill`, `border`, `title_fill`, `icon_fill`) live on `SvgNode` rather than in
    `SvgColors`, because a table with a `db_color` identity colour wears it on the canvas and an
    export that flattened every card to one grey would lose the only thing telling them apart at a
    glance. `icon_svg` inlines the UI's Lucide glyphs as nested `<svg>` elements and resolves their
    `currentColor` to a real value, which nothing in a standalone document would otherwise set.
    `ellipsize` is here but takes the measurer as an argument: the core has no fonts, and the
    truncation has to fall on the same character the card's own `text_ellipsis` falls on.
  - `monitor.rs` — the **Live Monitor**'s pure change detector: no DB, no timer, no UI.
    `Snapshot::from_result` captures a `ResultSet` keyed by its table's key columns (cells are
    `Option<String>` so NULL stays distinct from `""`), and `diff_snapshots` matches two snapshots
    by key into `RowChange::{Insert,Update,Delete}`, an update carrying per-column `FieldChange`.
    Row identity is just `Vec<String>`, so a new engine's fetch path only has to produce a
    `Snapshot`. The caller must skip the *first* poll itself — diffing against an empty prior reads
    every row as an insert. A delete carries the row's last-seen cells deliberately: it is the one
    case where the row is gone from the database and the log is the only remaining record.
    The **log** lives here too, not just the diff. `MonitorEntry { at, change }` — a change plus the
    `M:SS` at which a poll *observed* it — moved down from `ui/lib.rs`, which re-exports it so every
    use site still reads as a UI type, because the log's export is a pure projection of these
    entries and belongs beside the diff that produces them. `log_result_set` is that projection: it
    renders the log to a `ResultSet` so it exports through the ordinary `core::export` renderers
    instead of a second set written for it. One row per change — `Time`, `Action`, `Key`, then one
    column per watched-table column — with an insert carrying the new values, a delete the last-seen
    ones, and an update `old → new` in the columns that changed and the plain value everywhere else.
    A value standing alone is a real NULL cell, so each format spells NULL its own way; a NULL
    *inside* a transition can only be the literal text `NULL`, because the transition is one cell.
    **Width comes from the data, not from `cols`** — a change carrying more cells than the baseline
    named widens the result (the extra columns become `column_N`) rather than being truncated,
    because a silently narrowed export is the failure nobody notices later. `LOG_FORMATS` is
    everything the grid offers **except SQL**: SQL renders `INSERT INTO <table>`, and a change log
    has no such table — its rows are observations *about* one, a third of them deletions. Both caps
    are here for one reason, that the modal has to be able to *name* them: `ROW_CAP` (rows per poll,
    past which the monitor watches a page rather than a table) and `LOG_CAP` (changes kept, oldest
    dropping — the app's old private `MONITOR_LOG_MAX`, moved once the log became exportable, since
    a silently truncated record looks complete and isn't). **`trim_log` is the one place `LOG_CAP`
    is applied**, and it returns how many entries went, because the app trimming on `> LOG_CAP`
    while the modal's caveat printed on `>= LOG_CAP` meant a log resting exactly at the cap claimed
    a loss it hadn't had — on a record whose only value is that it can be trusted. The status line
    reads the count. `discard_needs_asking(len, exported)` is the other decision that belongs here
    rather than in a modal: the log is the only record of what a deleted row held and no poll
    re-reports a change, so throwing it away is irreversible — unless it is empty, or already on
    disk, which is why the confirmation isn't unconditional.
  - `activity.rs` — the **Server Activity** panel's model and every decision it makes about a
    snapshot of server sessions: no DB, no timer, no UI. `SessionInfo` is engine-neutral by
    construction (`id`/`user`/`client`/`database`/`state`/`sql`/`seconds`/`blocked_by`), so a third
    server engine only has to produce one. `SessionState` has four variants and **their declaration
    order is the attention order** — `rank` is derived from it, with one addition the states can't
    express: a session someone is *waiting on* belongs near the top whatever it is doing, because it
    is the one about to be killed. `prepare` is the single entry point the app calls on a fresh
    fetch: it derives each session's `blocks` list from the `blocked_by` edges (the engines answer
    only one direction), **deduplicates both directions**, sorts by rank → longest-standing → id,
    then cuts to `MAX_SESSIONS` and returns **whether it cut** — on the same rule as the live
    monitor's `trim_log`, since a silently shortened list looks complete and isn't, but as a `bool`
    rather than a count. The fetch asks for `MAX_SESSIONS + 1` rows precisely so this can notice,
    which means a server holding five hundred and one sessions and one holding four thousand arrive
    here identical: any number derived from them is `1`, and "500 sessions · 1 more not shown" in
    front of four thousand is a figure that looks precise and is off by three and a half thousand.
    `ActivitySummary::total_label(truncated)` prints `500+ sessions` for the same reason. The dedup
    is the other half of the same honesty: MySQL answers the blocking graph out of
    `performance_schema.data_lock_waits` (or `INNODB_LOCK_WAITS`), which is one row per *lock* pair
    rather than per transaction pair, so a holder sitting on both a record lock and a gap lock the
    waiter needs reported the same edge twice — "waiting on a lock held by 1148, 1148" on the waiter,
    and a "blocks N sessions" on the holder counting locks instead of sessions. PostgreSQL's
    `pg_blocking_pids` is already distinct, so doing it here is also what stops the two engines
    disagreeing. `render_slice` is the last cut, and a different one: the panel is rebuilt whole on
    every poll, so it draws the first `RENDER_CAP` of whatever survived the search and *counts* the
    rest (an exact figure — it is measured against the list in hand, not a server total nobody
    fetched). **The row→`SessionInfo` folds are here too** — `from_mysql_rows(rows, trx, waits)` over
    `MyProcessRow`, `from_pg_rows` over `PgActivityRow` (every `pg_stat_activity` column as the text
    `simple_query` hands back) — with the backends reduced to fetching and reshaping.
    **`PgActivityRow` is a named struct and its field order is `PG_ACTIVITY_COLUMNS`', which is
    also what the query projects.** `db::pg`'s `pg_activity_sql(limit)` builds the outer `SELECT`
    from that list, so the query and the fold are one list rather than two; `PgActivityRow::from_slots`
    destructures the fetched cells with a slice pattern and **refuses** a row that is not exactly
    eight of them. It was `[Option<String>; 8]` filled by `std::array::from_fn(|i| opt(r, i))`,
    indexed with bare literals — `text(r, 5)` for the query, `r[7]` for the blockers — against a
    `const` SQL string in another crate: `opt` answers `None` past the end of the row, so a
    projection that lost or gained a column folded silently into a session whose tail was all
    `None` — user empty, state `Idle`, blockers gone — and the panel whose one decision is what to
    kill went on working and stopped being true. Nothing failed;
    `every_slot_of_an_activity_row_is_the_column_the_query_selects` is what fails now, and it is
    written against the constant rather than against a helper that restates the positions. Each fold is a
    pile of decisions, not an I/O wrapper: which of MySQL's three result sets wins for a thread,
    whether a host becomes an address or nothing, whether a whitespace-only `INFO` is a statement,
    that a negative `TIME` clamps before it becomes a sort key, that PostgreSQL keeps a row the role
    may not inspect (`pid` and `pg_blocking_pids` stay real, so it is still killable and still a node
    in the graph) while dropping the `PG_MASKED_QUERY` sentinel so it is neither classified `Running`
    nor drawn as a statement, and that a row whose **pid** won't parse is dropped rather than
    admitted under a made-up id — a session `0` renders a full row with a live "Kill session" under
    it. None of that was reachable from a test while it sat inside an `async fn` holding a
    connection, written out once per engine beside the query that fetched the rows.
    The two classifiers are here rather than in the
    backends because they are judgements, not queries: `mysql_state(command, trx_state)` is why the
    transaction view is joined at all — a `Sleep` thread with a live `INNODB_TRX` row is a client
    that ran `BEGIN` and went away holding every row it touched, and `SHOW PROCESSLIST` alone cannot
    tell it from an idle pool connection; `pg_state(state, has_query, blocked)` promotes a lock wait
    (PostgreSQL reports one as an ordinary `active` backend) and falls back on whether a statement is
    visible for a backend the account isn't privileged to inspect. `lock_wait`/`lock_wait_text` pick
    the wait worth a banner and write its sentence — deliberately `None` when the holder isn't in
    the snapshot, since a banner offering to kill a session that isn't there is worse than no
    banner.
    **`SessionInfo::seconds` is `Option<f64>`, and `None` is not zero.** PostgreSQL masks
    `state_change`, `query_start` and `backend_start` for a backend the role may not inspect, so
    its age arrives NULL; folded to `0.0` it drew as **"0s"**, and a connection open for three
    hours claimed to have just arrived on the list a person scans for what has been sitting there.
    Every reader has an answer for the unknown rather than a substitute: `format_age` takes the
    `Option` and draws an em dash, both sorts put `None` last (`None < Some`, which is also what
    the query's `NULLS LAST` does), and `lock_wait_text` drops the duration from its sentence
    instead of inventing one. `KillKind` is an enum and not a bool because both engines offer both and the
    consequences differ in kind: cancelling leaves the session and its transaction alive, while
    terminating rolls back and drops the connection; `applies_to` withholds *cancel* from a session
    with nothing running, and `kill_confirm(kind, session, dialect)` writes the sentence that names
    which one is about to happen. It takes the dialect for **one clause, and it is the clause that
    decides whether the user's work survives**: MySQL's `KILL QUERY` leaves the transaction usable
    and the client can retry, while PostgreSQL's `pg_cancel_backend` leaves it open but *aborted* —
    every later statement fails with `25P02` until someone rolls back, so the uncommitted work is
    gone exactly as under a terminate. Telling a PostgreSQL user their transaction "stays open" is
    true and useless. `supports_activity` is the capability (**false for SQLite, which is a library in this
    process and has no other session to see**) and `supports_kill` is *computed* from it rather than
    spelling out a second `!= Sqlite`. `format_age` prints whole seconds under a minute because
    MySQL's `PROCESSLIST.TIME` has nothing finer, and steps the unit up rather than running the
    count on. The **poll interval is stored per connection** (`IntervalRule`, keyed by `conn_id`
    like the colour and favourite rules, in `ui_state.json`): it is not a taste but how hard you
    are willing to lean on a particular server, and one global number would carry a laptop's two
    seconds onto a production replica on the next switch. `interval_for` clamps on the way out and
    `set_interval` on the way in, so the picker can never be left with nothing marked; a choice
    equal to the default is still stored, or moving `DEFAULT_POLL_SECS` would silently move every
    connection someone had deliberately set to the old one.
  - `diff.rs` — `line_diff`/`build_diff_rows` (Ctrl+K preview).
  - `history.rs` — query-history model (`push`/`clear_conn`/`preview`/`relative_time`),
    persisted to `history.json`. An entry is written in **two passes** — `push` when the run
    launches, `finish` when it lands (duration, rows, `Outcome`) — because the two moments
    answer different questions: an entry has to exist while the query is still running (one the
    user cancels, or that the app doesn't outlive, is one they may most want back), and only
    the completion knows how it went. `finish` matches on `run_id` — an id the app hands out
    per launch from a counter that only goes up — **not** on `(conn_id, sql)`, which identifies
    the *statement*: two tabs can have one statement in flight at once, and keyed by statement
    the slower, older run overwrote the newer one's result on landing. Keyed by run it finds
    nothing, because `push` de-duplicated its entry away. Nothing to update is normal, not an
    error. `Outcome` has three states, not two — `Unknown` is what an entry starts in and what
    a cancelled run keeps, and the panel then shows no outcome line at all rather than guessing.
    Duration is the app's **wall-clock** around the whole call, not `ResultSet::elapsed_ms`: it
    is what the user waited through, and it is the same measurement for a failure, which is the
    case worth finding (a statement that spent 50 s behind someone else's row lock).
    `bucket`/`group_by_recency` are the panel's TODAY / THIS WEEK / EARLIER groups, split on
    **elapsed** time rather than calendar days — there is no timezone here, only millis, and a
    query run twenty minutes ago shouldn't leave "today" because midnight passed. They share
    `relative_time`'s arithmetic on purpose: a row's "3d ago" is read directly under its header,
    and a test pins the two together (exactly — the pin accepted either suffix under EARLIER and
    was therefore vacuous, since the two functions cross over at the same threshold anyway).
    `push` returns whether it recorded anything, so a credential-bearing statement doesn't cost a
    whole atomic rewrite of the file for nothing, and `drop_runs` deletes the entries of runs that
    never happened: a batch stops at its first failure and reports the rest `Cancelled` without
    dispatching them, so a 60-statement script failing at statement 2 evicted the connection's 50
    real entries in favour of 48 that never ran. Deliberately **not** applied to a single run —
    one the user cancels *was* dispatched and may have written something. `RunResult::loaded`/
    `failed` own the rows-vs-`affected` choice and the `rows_capped` rule, and `outcome_line` the
    facts line's composition, so both are in core rather than in a view builder: a wrong
    `rows_capped` writes a number into a log read long after the grid it came from. `preview`
    clamps at `PREVIEW_MAX` while `matches_query` searches the unclamped text — `max_height` and
    `clip()` bound *paint*, not layout, so a multi-MB `INSERT` was laid out whole on every rebuild
    of the panel, but clamping what is drawn must not become a decision about what is findable.
  - `health.rs` — connection health-poll policy: `tick(HealthCfg, TickCtx) -> Tick` decides
    ping-or-skip + the delay until the next tick (exponential `backoff` on consecutive failures,
    longer interval for SSH-tunnelled connections, skip while the window is unfocused / a query is
    already in flight / the tunnel isn't up). The app owns only the timer, `Db::ping` and the
    landing guard: a ping is up to five seconds of waiting, so `main.rs`'s `check_landing` stamps
    each check with `(connection id, generation)` and a check that lands after a switch — or after
    a newer check of the same connection — **writes** nothing: not `ConnStatus`, not the failure
    count `health::record` folds. Without it, leaving a dead connection for a healthy one repainted
    the header's "Disconnected · Retry" over a server that was answering, and only the next poll
    could clear it — the poll the same stale failure had just told to back off.
    **The `with_conn` continuation it may be carrying is a separate question, and asks
    `check_continues` — connection identity only, not the generation.** A write is a repaint, where
    a stale answer is worse than none; a continuation is somebody's click waiting on a reply, and
    dropping it doesn't leave the screen stale, it makes the control they pressed do nothing at all
    with no error — precisely what `with_conn` exists to prevent. Gating both alike made that
    promise false on a timer: a blocked action pings, the ping takes its five seconds against a dead
    host, and the health poll ticking (or the window regaining focus) inside that window stamped a
    newer generation and threw the user's own answer away. A superseded result of the *same*
    connection is still an answer about the same server and a few seconds old — enough to decide
    whether to proceed. A **different** connection still refuses, which is the case that matters:
    running an action gated on a server the user has left, or reporting the old one unreachable in a
    modal over the new one.
  - `window_chrome.rs` — which half of the window frame the app draws itself, now that it launches
    with `WindowConfig::show_titlebar(false)`. `Chrome::current()` answers per `Host`
    (Windows/Linux/macOS): `draws_own_controls`, `draws_own_resize_border`, `wants_drop_shadow`,
    `leading_inset`. **Ask the capability, never `cfg!(target_os = …)` at the use site** — the same
    rule the engines follow, for the same reason. The split is not cosmetic: floem reads that one
    flag as *undecorated* on Windows/Linux but as a *transparent* title bar over a full-size content
    view on macOS, so the traffic lights, the native resize border and the move behaviour all
    survive there. What macOS costs us instead is space — the lights are drawn over our header, so
    `leading_inset` reserves it. The Windows half is the one with teeth: winit strips
    `WS_CAPTION | WS_SIZEBOX` from an undecorated window, so without our own edge zones the window
    cannot be resized at all. `ui::window_chrome` draws what this module decides.
  - `tx.rs` — the **manual-transaction** state machine behind `TxMode::Manual` (no DB, no UI).
    Two engines only: **SQLite has no manual mode**, so the status-bar segment offering it is
    hidden on such a connection and `Session::open` refuses one — not because SQLite lacks
    transactions but because a pinned `rusqlite::Connection` is blocking and `!Sync`, needing a
    thread of its own and a channel, which is worth building deliberately rather than as a side
    effect of adding an engine. Running the tab's statements on fresh connections instead would
    break the single promise the mode makes.
    `TxState::on_statement(engine, sql, outcome)` folds one statement into
    `Idle`/`Open{stmts}`/`Poisoned{stmts}`/`Lost`. It is a state machine rather than a bool because
    the engines diverge: PostgreSQL aborts the *whole* transaction on any error (`Poisoned` — only
    `ROLLBACK` gets out), MySQL survives a failed statement but silently commits on mid-transaction
    DDL. `implicit_commit` is that list, read through the shared `leading_keyword` lexer; a **miss
    is not harmless** — after one, a Rollback runs as a successful no-op and reports an undo that
    never happened. `StmtOutcome::FailedIsolated` is what a `SAVEPOINT`-wrapped grid write reports,
    and it must not poison PostgreSQL, or one bad cell edit would tell the user their whole
    transaction died. `pill_text` is the status-bar string. It also owns what the user is told
    while a write **waits**: `write_blocking_tabs` (which of our own tabs' transactions a grid
    write could be queued behind — same connection scope as `ddl_blocking_tabs`, but excluding
    the writer's own tab, whose write runs *inside* that transaction) and `write_wait_note` →
    `WaitNote` (the sentence after `WRITE_WAIT_MS`, plus the one tab to offer a `ROLLBACK` for —
    `None` for several, since one button would have to pick and picking wrong ends a transaction
    the user didn't mean to). The sentence never names the tab; **the button does**, clipped by
    the bar — a custom tab name is arbitrarily long, and spelled into the sentence it pushed the
    button off the edge. Deliberately hedged: Schemaic doesn't track which rows a
    transaction touched, so an open one elsewhere is a *candidate*, not a diagnosis.
  - `format.rs` — per-column display formatters (`ColumnFormat`/`apply`: epoch→datetime, bytes,
    bool). Display-only; edit/copy stay raw. Persisted to `format.json`.
  - `connection.rs` — the saved-connection model. A `Connection` is a database **server**, not one
    database — the sidebar lists all of its databases. **SQLite is the exception to that whole
    sentence**: there is no server, so `Connection::file` is the entire target and
    `host`/`port`/`user`/`password`/`ssh` are inert, which in turn means no secret reaches the
    keyring (an empty password is *deleted* from the store, not written), no tunnel is opened, and
    the connection has exactly one database — the one SQLite calls `main`. `file` is
    `#[serde(default)]` so every connection saved before it loads unchanged, and it sits *beside*
    the server coordinates rather than replacing them because a connection's engine is editable in
    place: switching to SQLite and back must not discard the host it is going back to. Three
    consequences are written down where they are easy to get wrong — `endpoint()` shows the file's
    *name* (`host:port` would read `:0`, which looks like a misconfiguration), the split handles
    **both** platforms' separators since `connections.json` is portable and `std::path` on Linux
    returns a whole Windows path as its own file name, and `targets_same_server` counts the file,
    because pointing a connection at another `.db` reaches an entirely different set of tables and
    is reached the same way a repointed host is. `is_sqlite`/`is_postgres` are the one answer to
    which engine a label names — `schemaic_db::Engine::from_db_type`, the form's picker and
    `SqlDialect::from_db_type` all delegate, the last of which used to re-spell the aliases itself.
    `same_engine` is that pair asked of *two* labels — `MariaDB` and `MySQL` name one engine, as do
    `pg` and `PostgreSQL`, and as do `MySQL` and the empty label that predates the field — so the
    question is not a string comparison. It is what the connection form's Type picker tells its own
    change apart from a load with.
    `AiData` is the connection's **AI data-access level** — `SchemaOnly` / `OnRequest` (the
    default) / `Full` — and the single gate over every path that can carry this connection's rows
    off the machine: the `run_query` tool, `describe_table`'s sample rows, the grid's
    attach-to-chat, and the value sampling behind AI Summary / Fill / Seed. Per connection because
    a local scratch database and a client's production server are not the same risk, and one global
    answer forces the careless setting on one of them. It is `Option<AiData>` on `Connection`:
    `None` means "saved before the setting existed", which `migrated_ai_data` settles once at
    startup from the old global `ai_run_queries` flag, so an upgrade neither grants access nobody
    chose nor withdraws access that was working. What counts as *evidence* of that old flag is
    `persist::legacy_ai_run_queries_in(&[u8])` — split out of the `std::fs::read` because the
    decision is the whole point and the I/O put it out of reach. Four answers, three of them
    `None`: unparsable JSON, no such key, and a key holding something that is not a boolean. Only a
    recorded boolean is evidence, because what it decides is a one-way promotion of every saved
    connection to `AiData::Full`, and the migration never re-resolves. There is deliberately **no masking option** — a
    model cannot tell a masked value from a real one, so it reasons confidently about fiction, and
    the questions where values matter are exactly the ones masking ruins.
    `SshTunnel`/`SshAuth` cover the tunnel's own
    auth, including `Agent` (delegates to the running SSH agent, storing no secret at all).
    `ConnStatus::is_down` treats `Unknown` (not yet checked, or a tunnel still coming up) as
    *non*-blocking — only a confirmed failure gates work. `SshAuth`/`Environment`/`AiData`
    deserialize through a `…Raw` shim with `#[serde(other)]`, so a value written by a newer build
    degrades to a default instead of failing all of `connections.json` — and for `AiData` that
    default is deliberately the *safe* level, since an unknown one written by a newer build means
    more access than this one understands. There is deliberately no
    `mysql://user:pass@host` builder, and the password fields here aren't what's on disk — see
    `secrets.rs`. `duplicate` is the copy the connection list's right-click menu makes — a
    **struct update**, so a field added to `Connection` later is carried by construction; the
    failure mode is a credential silently not copied, which the field-by-field form would not
    fail to compile over. `targets_same_server` is the "is this still the same server" test the schema
    tree's reload gates on (see `schema.rs`'s `SchemaState::begin_refresh`) — everything that
    decides which server the next query reaches and nothing else, so a rename or a colour can't
    blank the tree and a repointed host can't leave another server's databases on it.
  - `schema.rs` — the introspected model. `ColumnInfo` carries the **full** column definition
    (type *with* parameters, default as ready-to-emit SQL text, auto-increment/identity, generated
    expression, `ON UPDATE`, comment, collation) because MySQL's `MODIFY COLUMN` replaces a column
    outright — anything not restated is silently destroyed, so a schema editor can't stand on a
    thinner model. `ColumnInfo::definition_sql` is that one emitter, shared by `CREATE` and (later)
    `MODIFY` so they can't drift. `identity_always` separates PostgreSQL's `GENERATED ALWAYS AS
    IDENTITY` from `BY DEFAULT`/`serial`/MySQL `AUTO_INCREMENT`, because only the first **rejects**
    an explicit value — `is_server_assigned()` is that question (`generated.is_some() ||
    identity_always`) and is what a write path must ask before naming a column. `IndexColumn` keeps prefix lengths + `DESC`; `ForeignKeyInfo`
    keeps its name (both engines drop by name) + referential actions. `ViewOptions` is the same
    idea for a view (check option, MySQL definer/security/algorithm, PG storage params +
    `materialized`) — `CREATE OR REPLACE VIEW` replaces the whole view, so what isn't restated
    resets, and `SQL SECURITY DEFINER → INVOKER` is a privilege change. `definer_sql` quotes the
    two halves of a MySQL account. **`column_list` is SQLite's alone and arrives by the opposite
    route**: the explicit `(x, y)` of `CREATE VIEW v (x, y) AS …`, held verbatim and without its
    parentheses, `None` on the two engines that bake the names into the body they report. It has to
    be modelled because *every* SQLite view edit is a drop-and-re-create
    (`ddl::supports_or_replace_view`), so a list left behind would silently rename the view's
    columns to whatever the body calls them; verbatim rather than a parsed `Vec<String>` because
    SQLite hands the list back with whatever quoting it was written with and re-quoting it is a way
    to change it. `TableInfo::create_ddl` — `CREATE TABLE`/`VIEW`, built on the
    above; its **view** branch delegates to `ddl::view_ddl` so Copy DDL, the MCP table-info tool
    and the apply path all emit through one view emitter (it used to have its own, which restated
    none of the options). **`TableInfo::implicit_key` is a capability, read rather than
    asked-by-engine**: the spelling of a row identity the table has that is none of its columns
    (SQLite's `rowid`), or `None` — which is every MySQL and PostgreSQL table, every view, and every
    SQLite `WITHOUT ROWID` table. `filter::table_query` reads it to decide whether to project it, so
    no caller has to test which engine it is holding.
    **`TableInfo::create_sql` short-circuits all of that where the engine keeps its own `CREATE`
    text** — which of the three only SQLite does (`sqlite_master.sql`, plus the `CREATE INDEX`
    statements it stores separately and without which a table's DDL is incomplete). That is a
    fidelity decision, not a shortcut: reconstructing a SQLite table from this model emitted
    `AUTO_INCREMENT`, which SQLite **accepts** by reading it as part of the type name — so the
    statement replayed silently into a different column — plus MySQL's inline `KEY name (cols)`,
    which SQLite cannot parse at all, and an empty column list for an index whose keys are
    `lossy`; and it dropped the foreign key, `WITHOUT ROWID`, CHECK constraints and column
    collations, none of which the model carries. It is deliberately **not** used for a *view*, even
    on SQLite: there the model is genuinely complete (a name and a body), so the shared emitter is
    both correct and consistent with the other engines. The test asserts SQLite takes its own DDL
    back, which is the only assertion that would have caught the `AUTO_INCREMENT` case.
    **`TableInfo::dependent_ddl` is the same fidelity call made for the rebuild**: the `CREATE` text
    of the objects that go down with the table and have to be put back — SQLite's triggers, filled
    by `sqlite::trigger_statements`, empty on the two engines that alter in place and so never
    destroy the table their triggers hang off. Deliberately the server's own statement rather than a
    re-emission from `TriggerInfo` — and that stays the call now that `sqlite::triggers_of` *does*
    read a SQLite trigger into the model. The two are not redundant: the model is what the **editor**
    diffs, the text is what a **rebuild** puts back without depending on the parse being perfect,
    and the failure mode the rebuild has to avoid is the one `IndexInfo::lossy` exists to prevent:
    the part that didn't survive the parse is gone from a trigger that still looks armed. Views need
    nothing here — `DROP TABLE` leaves a view that selects from the table in place, SQLite resolving
    a view's references when it runs rather than when it is declared, and the table returns under
    the same name before the transaction ends.
    **The tree's Generate DDL entries are two `DbSchema` methods, one per altitude.**
    `create_ddl_script(schema, dialect)` emits one namespace in **dependency order** — the
    standalone types, then base tables, then views, then the sequences that stand on their own —
    because an omitted foreign key leaves a script that still runs while an omitted type fails on
    the first `CREATE TABLE`; a sequence a `serial` or an identity column already creates is
    skipped, since restating it fails on a name that exists. `create_ddl_script_all(dialect)` is
    the database node's analogue: it walks `schemas()` in **display** order (`public` first, then
    alphabetical), so the script reads down the tree it was raised from, and joins only the
    namespaces that produced something rather than leaving a blank run where an empty one was.
    Where an engine has no namespaces at all — MySQL and SQLite, whose tables carry `None` — there
    is nothing to walk and it *is* `create_ddl_script(None, dialect)`
    (`create_ddl_script_all_is_the_flat_script_without_namespaces` pins that equivalence, which is
    the half a namespace-walking loop would quietly get wrong). Ordering *between* namespaces is
    consequently display order and not dependency order: a type used by a table in another
    namespace is emitted after it if the alphabet says so. That is the same class of gap
    `create_ddl_script` already carries for foreign keys, and it is accepted for the same reason —
    the script goes to the clipboard and an editor tab, is read and edited before it is run, and
    `ddl_preview` is still the only thing that runs anything.
    **`TriggerInfo`/`TriggerAction`/`TriggerEvent`/`TriggerEnabled`/`TriggerSource`** are the
    trigger half, and carry three rules the
    same "restate everything or it silently resets" logic as `ViewOptions`.
    `TriggerAction::Function::name` is **emittable SQL** on both producers — already quoted,
    qualified when it isn't in `public` — never a bare identifier; it once meant both
    depending on who wrote it, which bound triggers to `public`'s copy of a function picked
    from another schema. `TriggerEvent`'s **declaration order is load-bearing**: the derived
    `Ord` is what the UI sorts into and must be `pg_trigger.tgtype`'s bit order (`INSERT`,
    `DELETE`, `UPDATE`, `TRUNCATE`), not DML order, or an introspected trigger re-sorts into a
    phantom drop-and-recreate. `TriggerEnabled` is `tgenabled`'s **four** states, because
    `ENABLE ALWAYS`/`ENABLE REPLICA` folded into a bool get recreated as plain `O` and change
    what fires during replication apply; `old_table`/`new_table` are `REFERENCING OLD/NEW
    TABLE`, whose loss breaks *every write to the table* rather than failing the plan.
    `TriggerSource` is the MySQL body + session state, fetched lazily — see `schemaic-db`.
    `TriggerInfo::create_sql` is the one trigger emitter and has **three** arms, not two: SQLite's
    shape is neither of the others' — PostgreSQL's `UPDATE OF` and `WHEN` with MySQL's inline
    body, no definer, no ordering clause, no session state and always `FOR EACH ROW` — so it is
    asked for by name rather than reached by falling off the end of a `!pg`. `update_columns` and
    `condition` are consequently **not** PostgreSQL-only fields: MySQL is the engine with neither.
    `CheckInfo::validated`/`inherited` are PostgreSQL's `NOT VALID` / `NO INHERIT`, carried and
    restated: they are part of the clause, and `pg_get_constraintdef` prints them *after* the
    parens, which is why `ddl::check_predicate` must strip them before peeling. **An unnamed check
    stays unnamed**: `CheckInfo::clause_sql` writes a bare `CHECK (…)` when the name is empty,
    because most SQLite checks have none and `CONSTRAINT "" CHECK (…)` is not a nameless constraint
    but a syntax error — while inventing a name would make a rebuild read as though it renamed
    something.
    **`RoutineInfo`/`RoutineKind`/`SqlDataAccess`/`Volatility`/`RoutineSource`** are the stored
    functions and procedures, on both engines that have them, under the same rule again — a
    redefinition replaces the whole routine, so anything the statement doesn't restate reverts.
    Which fields those are is per-engine: PostgreSQL's `volatility`, `strict`, `language`, the
    per-routine `SET` clauses (a `SECURITY DEFINER` function that loses its pinned `search_path`
    is a privilege-escalation hole) and `identity_arguments` (the parameter list in the form that
    *identifies* a routine, which is not the form a `CREATE` takes — see `ddl.rs`), MySQL's
    `deterministic`, `data_access`, `definer` and `comment`. `security_definer` is the one field both engines have and **their defaults are
    opposite** — PostgreSQL's is INVOKER, MySQL's is DEFINER — which is why the MySQL arm of
    `create_sql` states the clause in *both* directions instead of leaving the default unwritten
    as the PostgreSQL arm does, and why `RoutineDraft::blank` seeds it per-engine rather than
    picking the safer-looking answer for both. `RoutineKind` is a tag rather than "is `returns`
    empty", because a function and a procedure are different objects to every statement that
    addresses one — and on PostgreSQL that is not only a keyword: **`CREATE PROCEDURE` takes a
    strict subset of a function's attributes**, so `pg_create_sql` withholds the return type, the
    volatility *and* the strictness for one, and `routine_editor` withholds the two controls that
    would set them. Any of the three on a procedure is
    `ERROR: invalid attribute in procedure definition`, and guarding only `RETURNS` left a plan
    that could be built in two clicks and could only fail at Apply.
    `RoutineInfo::is_editable` is the other gate on that model: the emitter writes the body as the
    routine's *source*, which is right for every language whose body is source text and wrong for
    `LANGUAGE c` and `LANGUAGE internal`, where `prosrc` is a link symbol and the recreate needs
    `AS 'obj_file', 'link_symbol'` — so those are listed and droppable but not editable, the call a
    materialized view gets.
    `RoutineSource` is the MySQL body + session state, fetched lazily, and exists
    for exactly the reason `TriggerSource` does — `information_schema.ROUTINE_DEFINITION` resolves
    the body's escapes, and every edit on that engine begins with a `DROP` that commits on its own,
    so a restate built from the resolved text can fail *after* the only copy is gone. Its `body` is
    an **`Option`** where `TriggerSource`'s is not, because the two halves of that row fail
    separately: the three session values need no parsing and are always trustworthy, so folding
    them into the body's success meant a routine with a header the reader didn't understand was
    later recreated under whatever `sql_mode` the applying session happened to have.
    **The standalone objects** — `EnumInfo`/`DomainInfo`/`SequenceInfo`, PostgreSQL's, plus the
    routines above — sit here beside the tables **on `DbSchema` itself** rather than being
    fetched lazily: the tree lists them and a column's type *is* one of them, so a separately
    refreshed second cache would be a second answer to "what is in this database" and the two
    would diverge on the first refresh that only updated one. `routines` used to be the exception,
    fetched only for the trigger editor's dropdown on the grounds that nothing rendered a body
    until an editor asked; browsing them is exactly the reader that argument said didn't exist, and
    their bodies are no heavier than the view definitions and trigger bodies this struct already
    carried. They are held as **`Arc<RoutineInfo>`**, and `ObjectItem::Routine` carries the same
    `Arc`, because `objects_all` is on the **keyboard-walk** path: `visible_nav_rows` rebuilds the
    whole row list on every arrow key, through `object_groups` → `objects_all`, so an owned clone
    there deep-copied every routine body in the database per keypress. Making the clone a refcount
    bump is the fix rather than giving `nav_rows` a cheaper borrowed view of the objects, because
    that walk has to stay bug-for-bug identical to the render and a second view of the same list is
    exactly how the two drifted before. An enum's `values` are in
    `enumsortorder`, not creation order, because that is the order comparisons use and the
    order `ADD VALUE … BEFORE/AFTER` manipulates. A domain's constraints are `CheckInfo`s —
    the same type a table's are, so `ddl::checks_equal` governs both. `SequenceOwner::internal`
    separates an identity column's counter (droppable only with the column) from a `serial`'s
    (an object in its own right), and `SequenceInfo::implicit_bounds`/`implicit_start` are why
    `create_sql` emits a clean statement instead of restating six clauses that say nothing;
    `last_value` is live state and deliberately takes no part in any diff. `qualified_ident` is
    the one "qualify unless `public`, then quote both halves" builder every one of these (and
    `RoutineInfo::signature_sql`) addresses its object through, and `find_by_ns` the one
    namespace-lookup rule behind every `DbSchema::find_*`.
    **`SchemaState::begin_refresh` is why a refresh doesn't blank the tree.** Re-introspection
    is whole-database on both engines and always will be — the cost is ~10 catalogue
    round-trips, not the rows, so scoping it to one table would optimise the term that doesn't
    dominate (measured: 48 ms for 600 tables / 12.6k columns on MySQL, 134 ms on PG, locally).
    Dropping to `Loading` for that window replaced every table and column row with one
    "Loading" row, after *every* DDL apply as well as every manual Refresh. So a database that
    has something on screen keeps it, and the method returns **`Option`**: a floem signal never
    dedups, so writing an equal `Loaded` back would dispose and rebuild the very subtree the
    refresh is meant to leave alone. Not writing is the only way to keep it. Nothing marks the
    row as busy meanwhile, deliberately: at these durations an indicator is a glyph flickering
    for a frame or two, which reads as a rendering fault rather than as progress.
    **There are two refresh paths and both go through it**, via the app's one
    `start_fetch` (`FetchSchemaFn`): the per-database one
    (`refresh_db`) and the connection-wide one (`load_schema`, the SCHEMA header's Refresh),
    which additionally **reuses the `ConnNode` of every database that is still there** — same
    `schema` signal, so the rows survive, and same node id, so the `dyn_stack` doesn't rebuild
    a surviving database at all. Reuse means the node `Scope` is kept rather than replaced, so
    it is gated on `Connection::targets_same_server` (id + engine + host/port/user + the SSH
    hop; *not* the password, name, colour or `read_only`): a **switch** must still clear and
    dispose, or the tree shows one server's databases while another's load — and a saved
    connection repointed at a new host keeps its id, which is the case an id comparison alone
    gets wrong. **Keeping the rows means `Loaded` stopped meaning *current*, and two things
    follow.** `main.rs`'s `fetch_landing` stamps each per-database fetch so an older one can't
    overwrite a newer (`try_update` guards a *disposed* scope, not a superseded fetch — press the
    connection-wide Refresh, apply an `ALTER`, and the pre-`ALTER` snapshot landed last and stayed,
    with nothing to detect it and no further refresh scheduled). And `ConnNode::refreshing` makes
    "a re-introspection is in flight" askable, which `table_designer::loaded_table` — the one
    funnel all four schema editors go through — answers `None` for: seeding a draft from a
    pre-apply `TableInfo` is what makes MySQL's `MODIFY COLUMN` silently restate the old column
    definition, and `risks()` discloses nothing because from the plan's view nothing changed.
    The plan behind the reuse is `plan_nodes`, pure and tested: it decides that a dropped and
    re-created database gets a **fresh** id rather than colliding with a live node, that reordering
    the server's list renumbers nothing (the tree keys on id), and that a reload against an empty
    list still works.
  - `secrets.rs` — keeps connection secrets (DB/SSH passwords + SSH key passphrase) out of the
    plaintext `connections.json`: the `SecretStore` seam + pure transforms `hydrate_file` (load →
    fill empty fields from the store, flag legacy plaintext for migration), `sanitize_file` (save →
    move secrets into the store, blank the disk copy; keep plaintext only if the store is
    unavailable) and `forget` (delete). The real keyring-backed store lives in `schemaic-app`'s
    `secrets` module (the heavy `keyring`/D-Bus dep stays out of core); pure + unit-tested via an
    in-memory fake.
  - `rowjson.rs` — the per-field model behind the grid's whole-row **view/edit panel**. A `ColSpec`
    per result column carries what the panel needs (name, editability, nullability, current value);
    `field_value_text` renders a cell into its editable text and `update_changes` diffs the panel's
    per-field state back into the changed *editable* columns for an `UPDATE` (rejecting a read-only
    edit or a NOT-NULL→null, and treating an untouched field as a no-op via normalized-text
    compare). Validation here covers NULL and read-only rules only — **type-correctness stays the
    DB's job**, and its error surfaces in the panel. Pure + unit-tested.
  - `jsontree.rs` — the editable JSON tree the row panel uses for a `json`/`jsonb` column.
    `JsonNode::parse` walks the document as `RawValue`s rather than going through
    `serde_json::Value`, for two reasons: object key order survives, duplicates included (a
    PostgreSQL `json` column really can hold `{"a":1,"a":2}`), and **a number keeps its source text
    byte for byte** — going through `f64` turned `10.00` into `10` and a 21-digit integer into a
    different integer, silently, the moment an unrelated leaf edit re-serialised the tree.
    `PathSeg::{Member(i),Index(i)}` addresses by *position*, not key, so duplicate keys get distinct
    independently-editable rows; `set_leaf` reparses arbitrary JSON, so editing a leaf may change
    its type. `rows`/`TreeRow` is the flattened outline the UI renders.
  - `plan.rs` — `QueryPlan::from_result` parses an `EXPLAIN` result into a table + heuristic
    warnings (full scan / filesort / temp table); `to_prompt_text` for the AI.
  - **AI prompt + reply plumbing** (all pure, all dialect-aware — a prompt that hardcodes
    "MySQL/MariaDB" asks a Postgres connection for backtick-quoted SQL the server rejects):
    - `prompt.rs` — fences DB content so an embedded ` ``` ` can't escape into prose. Every prompt
      built from server-controlled text goes through it. It also owns **what the assistant is told
      about the result on screen**: `result_shape` renders a `QueryState` as columns + types, row
      count, cap, elapsed, database and (on a failure) the engine's error verbatim — *never a cell
      value* — while `result_attachment` renders the rows the user deliberately attached, capped at
      `ATTACH_ROW_CAP` with the cap stated in its own header so a model handed 200 of 5,000 rows
      can't mistake the sample for the set. **The error text is the one arm gated on `AiData`**, and
      `result_shape` takes the level as an argument for it: an engine's error is not shape.
      `Duplicate entry 'alice@corp.com' for key 'users.email'` is a stored cell quoted back by the
      server — so below `may_query` the failure is still reported and the text withheld, naming the
      setting so the model asks the user to paste it rather than retrying blind. **The gate is
      `may_query` — `Full` alone — and not `may_attach`**, because the reason is level-independent
      and `Full` is the only level whose consent covers a value the user did not hand over:
      `OnRequest` is the default and its consent line reads *"Rows you attach from a result leave
      this machine with that question"*, and nobody attached that one.
      `the_engines_error_leaves_only_where_the_consent_line_covers_it` asserts the property over
      every level rather than one, so a fourth level can't be added on the wrong side. Both build on `pipe_table`, the one
      table renderer for anything a model reads (the MCP tools call it too), so a cell containing
      `|`, a newline or a megabyte of JSON is handled identically wherever it appears.
    - `summary.rs` — the grid's "AI Summary" cell/column prompts. `sample_column` spreads its
      sample *evenly* across loaded rows rather than taking the first N, because a sorted result's
      head often shares a date/status/prefix that reads as a pattern that isn't real; `sample_row`
      supplies the focused cell's own row, since a lone value rarely explains itself. Samples only
      what is already on screen — no round-trip — so the menu action is instant.
    - `seed.rs` — AI-generated seed data (Fill Value / Seed Table). `build_fill_prompt`/
      `build_seed_prompt` assemble the one-shot prompt from the table's DDL plus a *bottom* sample
      of real rows, so enum/format/FK conventions come from data rather than guesswork;
      `parse_fill_response`/`parse_seed_response` read the reply back (fence stripping,
      case-insensitive bare `null`, JSON bool → `"1"`/`"0"` for MySQL `tinyint(1)`).
    - `propose.rs` — the AI's proposed table change, as a **patch**: `Proposal`/`ProposedOp`
      deserialize the model's JSON (`{"add_column": {…}}`, externally tagged, `deny_unknown_fields`
      so an invented key fails loudly instead of being dropped), and `apply` lays the ops over
      `TableDraft::from_table` to produce a draft that then goes through `ddl::diff` → `emit` → the
      preview modal like the designer's own. So the model never writes SQL, never writes a
      `ChangeSet` — `ddl::diff` stays the only differ — and the write stays behind the same Apply
      click. **A patch rather than a whole-table draft**, because `diff` compares field by field:
      a model that re-types `varchar(255)` as `VARCHAR(255)` would propose a `MODIFY COLUMN` nobody
      asked for, and one that omitted a column would propose to **drop** it, with nothing
      downstream able to tell either from an intended change. What a patch doesn't name, it doesn't
      touch. Ops compose through the draft's own mutators (`rename_column`, `remove_column`), so a
      rename carries its indexes and keys and a drop cascades to them; the result is run through
      `TableDraft::validate` before it is returned, so the designer's rules stand between the model
      and the preview. Column lookups match case-insensitively — a model routinely writes `Email`
      for `email` — and keep the server's spelling.
      **`resolve_target` is the one resolver for *which table* a proposal is about**, and it is one
      function because the two ends of a proposal have to agree: the MCP tool checks the ops against
      a table and answers "Valid. Nothing has run.", and the card then builds the plan the user
      consents to. They read the same JSON by different rules — the tool ignored `proposal.schema`
      while the card required an exact match — so on a database holding both `public.orders` and
      `sales.orders` the model was told its change was valid for one table and the user was offered
      it against the other, and the qualified `sales.orders` form the tool's own description asks for
      dead-ended at the card. The order is the one a caller means: an explicit `schema` field, then a
      qualifier written into `table`, then the bare name — which `DbSchema::find_table` already
      resolves preferring `public`, so the ordinary PostgreSQL case needs no separate default. **It
      refuses a view**, which is the other half of being one function: `DbSchema::tables` holds views
      too, every earlier caller of these lookups came from a tree row whose kind the user had already
      seen, and a view laid under `TableDraft::from_table` emits `ALTER TABLE` — which PostgreSQL
      *accepts* for a rename, under a modal that says "Rename the table".
      **`apply`'s own guard has to decompose the name the way `resolve_target` does**, which is
      `names_the_same_table`. It compared the raw `proposal.table` to the bare `current.name`, so a
      qualified `sales.orders` resolved through the paragraph above and then dead-ended one line
      later — on exactly the databases with more than one namespace, where the listing prints the
      name that way and `propose_table_change`'s own description asks the model to write it. The
      guard is not dropped, because it still protects the case it was written for: a caller that
      opened a table itself and never resolved anything. So the namespace is enforced **where the
      resolver enforces it** and nowhere else — an explicit `schema` field is an exact lookup
      there, while a qualifier written into `table` is a first attempt the resolver falls back
      from, and on MySQL `mydb.orders` legitimately lands on the unqualified `orders`.
      `FENCE_TAG`/`is_proposal_tag`/`parse` are the *carriage*: a proposal reaches the user as a
      fenced block tagged `schemaic-proposal`, which `ui::markdown` renders as a card. Its own tag
      rather than `json`, because a model discussing a schema prints example JSON constantly and
      none of it is consent to edit a table — an offer has to be something the model made on
      purpose. **End to end:** the model calls the MCP `propose_table_change` to check itself,
      echoes the same JSON into its reply, `markdown::proposal_card` renders it,
      `ddl_preview::preview_proposal` turns it into a plan, and the user clicks Apply. Every step
      before that last one is read-only.
    - `transcript.rs` — the rendered shape of one AI turn (`ChatMessage`/`Seg::{Text,Tool}`/
      `TurnStats`), kept here rather than in `schemaic-ai` so the UI crate needn't depend on the
      CLI-integration crate. `ChatMessage::prose` is what copy *and* conversation replay use.
      A user turn may carry an `Attachment` — the result rows sent with that question. Its
      `summary` persists so a reopened conversation still shows that data went with the turn, but
      the cells are `#[serde(skip)]`: sending the rows was the consent, keeping a client's customers
      in `chat.json` afterwards is a second thing nobody asked for. `retained()` is how a view tells
      a live attachment from a restored one. `total_rows` is the count **before** the cap and is not
      derivable from `rows`: the builder caps on the way in, so a header computed from what survived
      reported "200 of 200" for a 5,000-row selection and handed the model a sample it read as the
      set — the cap notice exists precisely for that case, so the number it needs has to travel.
      `fingerprint` folds the attachment in, per its own rule that a field a mutation can change
      belongs there.
      Also the message box's **prompt recall** (Ctrl+Up/Down): `user_prompts` (the user's own
      questions, newest first, blanks dropped and a repeat kept only at its newest spot) +
      `recall_step`, which is a **cycle** — `None → newest → … → oldest → None` — rather than a
      list that stops at its ends, because the empty box is the only way back out of a recall and
      both keys have to reach it. A cursor past the end of a conversation that has since changed
      reads as `None`, so a stale walk restarts instead of landing somewhere arbitrary. The
      **gating** is `recall_apply` → `Recall::{Refuse, Show}`, here beside the arithmetic rather
      than in the view: it trims, like the send icon and Enter and like `user_prompts` itself, and
      the view's own copy did not — so one space in the box refused the recall silently, the row
      disagreeing with itself about whether anything was typed.
      `ChatMessage::fingerprint` is what the panel's per-message memo compares. The memo held a
      whole `ChatMessage`, so every streamed chunk deep-cloned and deep-compared all N of them —
      over segments that include a tool call's untruncated result — and kept a permanent second
      resident copy of the conversation. It is `O(segments)` and allocates nothing, on the stated
      assumption that a message is only ever *extended*; a test walks every mutation the stream
      makes, because a fingerprint that misses one freezes a bubble's content on screen.
    - `chat.rs` — per-connection conversations persisted to `chats.json`. `ChatFile::of` replaces
      every tool `result` with `RESULT_OMITTED` before it reaches disk — a `run_query` result is up
      to 200 rows of real table data, and writing it verbatim exported user data to a plaintext
      config file indefinitely. Stripping happens at the *bytes* boundary, not in `save`, so
      switching connections and back mid-session still shows that session's own results.
      `persistable` drops a still-streaming turn and its unanswered question before capping. A
      restored conversation is transcript, not memory: prose is replayed into a fresh `claude`
      session's system prompt, tool calls never are.
  - `text_ops.rs` — Ctrl+/ `toggle_line_comment` + `find_matches`/`replace_all`/
    `contains_ignore_ascii_case` (find bars). Pure, ASCII-case-insensitive, byte-offset-preserving.
    `selected_text` resolves a mirrored byte range against the buffer for the AI panel: the range
    comes from the mounted editor while the text comes from the tab's own signal, so the two can
    disagree by a keystroke — an empty, reversed, out-of-range or mid-character range yields `None`
    ("no selection"), never a panic.
  - `sqlfmt.rs` — `format_sql` (Ctrl+Alt+L pretty-printer): re-flows whitespace/indent/line-breaks
    "block" style, **preserving keyword case**; built on `skip_noncode` so comments/strings/backtick
    idents pass through untouched; indent follows editor tab-width/soft-tabs.
  - `pairs.rs` — caret-driven, boundary-aware editor highlights + auto-close pairs (via
    `skip_noncode`): `auto_pair` (auto-close `()`/`''`/`""`/`` `` `` [MySQL] at code positions, wrap a
    selection, type-over a closer/quote already at the caret — respects string/comment regions and
    word-adjacency guards), `backspace_pair` (delete both halves of an empty pair), `match_paren` (the
    paren adjacent to the caret + its partner, ignoring parens in strings/comments),
    `identifier_occurrences` (every whole-word, ASCII-case-insensitive occurrence of the identifier
    under the caret — excludes keywords/numbers/strings, needs ≥2 to fire), and `region_at`
    (`Code`/`Str`/`Comment` classification). Pure + unit-tested; dialect-aware (no backtick on PG).
  - `sqlfile.rs` — the pure half of opening and saving a tab's SQL as a `.sql` file on disk: what a
    file's bytes become in the editor (`decode`), what the editor's text becomes on the way back
    (`encode`), what the tab is called once it has a file (`tab_title` — the file name *with* its
    extension, or `orders.sql` and `orders.txt` would give two tabs one title), and what the Save
    dialog should suggest for a tab that hasn't got one (`suggested_name`), plus the
    `SQL_EXT`/`SQL_EXTENSIONS`/`SQL_FILTER_NAME` both dialogs filter on.
    **Line endings are remembered, not normalised away.** The editor works in `\n` and a `.sql` file
    checked into a repository on Windows very often does not, so a save that rewrote every line
    ending would turn a one-line edit into a whole-file diff — the kind of change that survives
    review only because nobody can read it. `decode` collapses `\r\n` and *records* that it did, the
    flag rides on the tab and on its `persist::SavedTab`, and `encode` puts it back (guarding
    against a `\r\n` pasted into the buffer doubling into `\r\r\n`). Mixed endings resolve to the
    majority and a tie to CRLF, because normalising the odd stray LF is the smaller lie; a lone `\r`
    is not a line ending at all and is left exactly as it is, since that is what a string literal
    may legitimately hold. `decode` also strips a UTF-8 BOM — a Windows tool writes one and it
    arrives as an invisible character in front of the first keyword — and `encode` puts that back
    too, on the same argument: dropping it rewrites the file's first three bytes for a one-line
    edit. Both flags, and one more, ride on `SqlFormat`, which is what the tab and its `SavedTab`
    carry.
    **A lossy read is not a licence to write.** `decode` reads bytes it cannot make sense of as
    U+FFFD, because a mis-encoded byte should cost a replacement character rather than the whole
    file — and that is a decision about *reading*. Writing the result back is a different act: it
    replaces every unreadable byte in the file permanently, including in the thousands of lines
    nobody touched, and a Latin-1 `mysqldump` is the ordinary shape of it. So `SqlFormat::lossy`
    records that it happened (free — `from_utf8_lossy` already allocates only when it substituted),
    persists across a relaunch, and the app confirms before such a save. The `.sql` file is the one
    artefact in this application Schemaic cannot regenerate, which is also why the write is
    `persist::write_file_atomic` (stage beside, rename over — no `.bak` and no mode change, since it
    is the user's own file) and why the save re-reads the bytes immediately before the rename and
    refuses when they are not what the tab read.
    **Size is asked before the bytes are.** `open_verdict` confirms past 1 MB and refuses past 64
    MB, and the reason is the editor rather than the read: `fs::read` and `decode` are cheap even at
    256 MB, while `intel::diagnostics` runs over the whole document on the UI thread 120 ms after
    every pause in typing — measured 710 ms at 1 MB, 11.4 s at 16 MB, 44.8 s at 64 MB, on an *empty*
    catalogue.
    The naming half is entirely about what a file system will refuse: `suggested_name` maps the
    characters Windows forbids to `-`, trims the trailing dots and spaces it also refuses, appends
    `.sql` only when it isn't already there (a tab opened from `orders.sql` must not suggest
    `orders.sql.sql`), and falls back to `query.sql` when nothing alphanumeric survives — a title of
    `///` scrubs to `---`, which is legal and useless. `ensure_extension` fills a *missing*
    extension only: the native dialogs mostly append the filter's own but not on every platform,
    while `schema.ddl` is the user saying what they want and quietly writing `schema.ddl.sql`
    instead is worse than honouring it. Pure + unit-tested inline.
    **Whether two paths are one file is asked here as well** (`same_file`, over `same_path` with
    `PATHS_IGNORE_CASE`), because two tabs bound to one file is a lost edit: each keeps its own copy
    of the bytes on disk, so saving the second discards the first — and the first goes on reporting
    itself clean, since its own copy still matches what it wrote. The comparison that allowed it was
    `Path`'s `==`, which is component-wise and case-**sensitive** on every platform, wrong in exactly
    the direction Windows and macOS need. The fold is a `const` per platform rather than a probe (a
    wrong answer merges two files the user meant to keep apart), and both readings are tested. It is
    deliberately only the path-shaped half: the app canonicalises first — which settles case, 8.3
    short names, junctions and a substituted drive when the file exists — and asks this afterwards,
    for the path being saved to for the first time, which cannot be canonicalised at all. The Open
    command searches **every** tab through it, not the active connection's: the strip being
    per-connection is a fact about visibility and no answer to the lost edit, and a hit on another
    connection goes through `switch_conn` so the strip shows the tab it activates.
    **A tab's binding to a file is one value, not three fields.** `FileBinding { path, disk_sql,
    format }` with `restored_binding(path, file_dirty, query, format)` and `FileBinding::none()`,
    because both failures here are a line left out. The file's *text* is not persisted, so whether
    a restored tab knows its on-disk copy has exactly one answer per saved tab: a tab saved clean
    had its editor text equal to the file and that text *is* persisted, while one saved dirty leaves
    it unknown — and getting the fourth combination wrong brings a dirty tab back looking clean, so
    the modified marker is gone and Ctrl+S is a no-op over unsaved work. `none()` is the other side:
    the blank slate left when a connection's last tab closes must shed the path (or the next Ctrl+S
    overwrites that file with an empty document) *and* the format (or a fresh script is written with
    a BOM and CRLF it never had). `app/main.rs` reads both at its two sites; the decisions
    themselves are tested here.
  - **Small persisted / UI-state models**, each a flat `Vec` keyed by `conn_id` and each pure +
    tested (they share `history.rs`'s shape; a new one belongs here, not in the UI):
    - `search_history.rs` — recent Find-Anywhere targets (`MAX_PER_CONN`, newest-first, deduped).
      `push` records only an *activated* result, not every keystroke, and the PG namespace is part
      of the dedup identity so same-named tables in two schemas don't collapse into one. So is
      `ObjectTag`, which is what makes an entry an enum/domain/sequence/function/procedure rather
      than a table (its name rides in `table`, so every file written before objects were searchable
      still loads): a type and a table may share a name in one namespace and are different places
      to go back to. The tag is a **persisted** enum of its own rather than `ddl::ObjectKind` so a kind
      written by a newer build degrades instead of failing the file and losing every connection's
      history, the rule `SshAuth`/`Environment` follow; it resolves to no live kind, so the row is
      dropped from the recents list exactly as an entry for a since-renamed table is. `Unknown`
      **keeps the text it didn't recognise** and a hand-written `Serialize` writes it back
      verbatim — the obvious `#[serde(other)]` unit variant degrades the *file* but silently
      destroys the *value*, and since the app rewrites the whole of `search_history.json` on every
      change, merely running an older build once would rewrite a newer one's `"collation"` as the
      literal `"unknown"`.
    - `favorite.rs` — the `(conn_id, database)` star list. `toggle` appends newest-**last** on
      purpose: `rank` (0 = that connection's oldest) is what the schema tree sorts by, so order in
      the `Vec` *is* the sort key.
    - `db_hidden.rs` — the `(conn_id, database)` set behind the SCHEMA panel's eye, the third store
      on that context menu and the last one to be keyed by connection. A flat `Vec<String>` of bare
      names meant hiding `world` on one server hid a live `world` on every other — out of the tree,
      Find-Anywhere, the toolbar selector, autocomplete, the assistant's context and `list_schema`'s
      overview — and left a deleted connection's names hiding databases forever, since there was
      nothing to `clear_conn`. What every consumer reads is still a `HashSet<String>`: `names_for`
      resolves the rules for the connection being looked at, which is the question each surface is
      asking and what keeps `schema::db_visible` a two-argument predicate. `migrate_flat` reads a
      file written before this and applies its names to **every** connection — the only honest
      reading of a set that had no connection dimension, since re-scoping it to whichever connection
      happened to be active would silently unhide databases on all the others.
    - `db_color.rs` — identity colours: a per-`(connection, database)` one and a
      per-`(connection, database, table)` one. Display-only — a dot in the tree, the active-DB
      selector and tabs for a database; a dot on the table row and a tint on that table's card
      header in the ER diagram for a table — and **manual only**, never inferred, and explicitly
      not the editor's production-red danger frame, which stays a *connection*-level signal. The
      two are **separate stores in one file** (`DbColorsFile { rules, tables }`, `db_colors.json`),
      not one store with an optional table: nothing then has to remember to filter the database
      rules out of a table lookup, and a database named `app` cannot lend its colour to a table
      named `app` — which is what `database_and_table_colours_are_separate_stores` pins. `tables`
      is `#[serde(default)]`, so a file written before table colours existed still loads. Sharing
      the file is why the app builds **one** `save_db_colors` for the pair, since writing either
      half alone would drop the other, and why deleting a connection runs both `clear_conn` and
      `table_clear_conn`. A table's key is its **display name** (`schema::TableSource::display` —
      the bare name on MySQL/SQLite and inside PostgreSQL's `public`, `schema.table` outside it)
      rather than a separate `schema` field, because that spelling is already the identity an
      ER-diagram node id carries, so the diagram looks a colour up by node id with no reparsing.
    - `tabsel.rs` — tab-selection rules for a strip that shows only the active connection's tabs, so
      every question (`pick_active`, `neighbor` after a close, `cycle`, `closing_would_empty`, `nth`
      for Ctrl+1‑9) is answered *within one connection*. `nth` especially: the Nth visible chip is
      not the Nth entry of the flat `Vec` once another connection's tabs interleave.
      `pick_active` prefers the remembered per-connection tab, so switching away and back doesn't
      dump the user on tab 1. `can_close`/`all_to_close`/`others_to_close` are the **closing** half,
      over a `ClosableRef` that carries the pinned flag as well (a pinned tab is visible and
      selectable but not closable): they are here rather than in the menu builder because the entry
      has to *dim* on the same expression the action evaluates — "Close other tabs" was always
      enabled and silently returned on a connection with one tab, one row below an entry that is
      dimmed for the same kind of reason. `can_close` is the single-tab form, and the app's
      `guard_close` asks it *before* prompting: one rule, two shapes, held together by
      `all_to_close_is_every_closable_tab`, because a tab the menu offers and the gate refuses (or
      the reverse) is a click that does nothing.
      `scoped_database(tab, active_conn, fallback)` answers the other question about the focused
      tab: the database a request about "the current tab" should run against — the tab's own, but
      **only when that tab is on the active connection**, otherwise the caller's already-scoped
      fallback. Switching connections doesn't move the focused tab and a tab keeps the connection
      it was opened on, so the focused tab routinely names a database that lives somewhere else;
      handing that name to the active connection's `Db` is how the MCP endpoint came to ask MariaDB
      for `chinook`. It is here rather than in each of its three callers — the AI's turn context
      (`app/ai.rs`), the terminal's DB-CLI button (`app/main.rs`) and the AI proposal card
      (`ui/ai_panel.rs`) — because the third is not harmless: the answer goes to
      `ddl_preview::preview_proposal`, which pairs it with `edit_ctx`'s *active* connection and
      stamps that `conn_id` into the plan `run_ddl` executes, so getting it wrong runs an `ALTER`
      on prod from a proposal written about dev. With no database to name, the card refuses and
      says to switch to that tab first. That copy had the rule
      spelled out inline, expression for expression and untested, because `schemaic-ui` cannot
      depend on `schemaic-app` — which made it a misplaced function rather than an unavoidable
      duplicate.
    - `palette.rs` — parses the command palette's `>` command mode into
      `Parsed::{Search,Filter,Command{name,arg}}`. The hard part is when typing stops filtering the
      command list and becomes an argument: longest-word-prefix match against the caller's
      argument-command names, under an invariant the caller must uphold — no argument-command name
      may be a word-prefix of another (`indent style`/`indent width`, never a bare `indent`).
    - `resource.rs` — the status bar's CPU/RAM model. `ResourceSample::new` divides `sysinfo`'s
      per-process CPU% (single-core-relative, so it exceeds 100 on a multi-core box) across the
      logical core count to give a whole-machine 0..=100. Sampling itself stays at the app boundary.
    - `update.rs` — the auto-update state model: `resource.rs`'s neighbour in spirit, and the pure
      half of the Velopack plumbing in `schemaic-app`'s `update.rs`.
      `check_gate(opt_out, installed)` answers whether a check round may run at all, and both of its
      refusals are ordinary outcomes rather than errors, so neither is shown to anyone: `OptedOut`
      (`SCHEMAIC_NO_UPDATE_CHECK`, read through `opt_out_requested`, which accepts `1`/`true`/`yes`/
      `on` case-insensitively and nothing else — a `=0` left in a shell profile means what it says,
      and the empty string a bare `set VAR=` leaves behind is not an opt-out) and `NotInstalled` (a
      portable-zip extraction, a `cargo run` dev build, a distro package). `UpdateState` is what the
      header's update chip renders, and two of its rules are why it is a type rather than a pair of
      signals.
      **`label()` returns `None` for `Idle`, `Checking` *and* `Failed`** — a background check that
      finds nothing, or that cannot reach GitHub, is completely invisible, so the header looks
      exactly as it did before the feature existed for most of most sessions; the failure is logged,
      not surfaced, and the next round retries — which is only worth anything because `app::logging`
      writes that line to a file, since on an installed build it used to go to a console that does
      not exist. And **`with_progress` only mutates `Downloading`**:
      Velopack's progress channel can still deliver a tick queued behind the end of the download, and
      folding it in blindly would replace "Restart to update" with "Updating… 100%" — a dead chip
      where the offer the user was about to click had been
      (`a_late_tick_cannot_rewind_a_staged_update`). `clamp_pct` is there because that channel
      carries a plain `i16`, so nothing at the type level stops a `-1` sentinel or a `101`.
      `should_recheck(gate, settled)` is the pure half of the *periodic* check, and is true only when
      the gate was `Allowed` and the round settled to `Idle` or `Failed`. Checking used to happen
      once per launch: v0.16.0 was left running while v0.16.1 was published and still showed nothing
      two hours later, and restarting found the update immediately — for a database client people
      leave open for days, launch-only checking undercuts the point of shipping auto-update at all.
      Two outcomes end the polling permanently. A **staged** update, because there is nothing better
      to find until the user restarts and another round could only disturb the offer already on
      screen; and a **refused gate**, because neither `OptedOut` nor `NotInstalled` can become
      `Allowed` inside a running process, so on a dev build it would otherwise be a timer that can
      never do anything (`recheck_stops_once_something_is_staged`, `a_refused_gate_never_re_arms`). A
      *failed* check does re-arm — it is the one negative outcome expected to be temporary, and it is
      invisible anyway, so retrying costs the user nothing.
    - `text.rs` — `plural(n, one, many)`, returning only the noun form so a humanized count
      (`"1.2k"`) can be displayed while the singular/plural decision still follows the true `n`;
      `human_count` (`1250` → `1.25k`), the **row-count** printer, shared by the grid's stats line
      and the properties surface so `200k` means one thing — and bound by a round-trip property:
      every string it emits must parse back through `model::goto_row_index`. Not the namesake in
      `transcript.rs`, which buckets token counts differently on purpose.
    - `stats.rs` — table statistics: `TableStats`/`IndexStats`/`SchemaStats`, `format_bytes`
      (1024-based, IEC units), `format_age`, and the honesty types. Every figure is an `Option`
      because the three engines publish different facts — `supports_table_stats` is `false` for
      SQLite, which keeps no per-table size at all. `RowCount::{Exact,Estimate}` stops an estimate
      from printing as a fact, `Freshness` says *why* a figure may be stale (MySQL caches
      `information_schema` stats for `information_schema_stats_expiry`, a day by default;
      PostgreSQL's `reltuples` is only as fresh as the last `ANALYZE`), and `IndexStats::is_unused`
      flags an index only when the server actually counted zero scans — `scans: None` is "nobody
      was counting", never "drop this". `count_rows_sql` builds the exact-count statement here
      rather than three times in the db crate, through the one quoter (`export::ident_sql`).
      Filled by `Db::fetch_table_stats`; rendered by `ui/properties.rs`, the schema tree's
      size column, the results toolbar and the destructive confirmations.
      - **Where a figure may be printed, and in what words, is decided here too** — the surfaces
        that print one hold no rules of their own. `catalogue_key` says where a statement's
        `qualifier.table` sits in the catalogue, because the two engines that publish statistics
        disagree about what a qualifier *is* (a database on MySQL, a namespace on PostgreSQL) and
        getting it backwards reports one table's figures for another with nothing to show that it
        happened. `SchemaStats::find` then resolves the name a *statement* wrote — unlike `get`,
        whose `None` namespace means "this engine has none", `find`'s means "the statement didn't
        say", so it matches by name alone and **only when exactly one table carries it**: an
        unqualified PostgreSQL name resolves through `search_path`, which the client does not know.
      - `rows_read_of` is the toolbar's row segment (`1k` alone, or `1k of ~4.2m`), and it drops a
        total at or below what was already read — `1k of ~400` reads as a bug rather than as the
        stale estimate it is. `truncate_prompt`/`drop_prompt` are the destructive confirmations'
        wording, and they name a figure only above `CONFIRM_ROW_FLOOR` (1,000) when it is an
        *estimate*: the point of naming one is scale, and InnoDB's sampled `TABLE_ROWS` is at its
        least reliable exactly below that — it reports 0 for a table holding a handful of rows, so
        *"Delete all ~0 rows in orders?"* would answer a question the user didn't ask, wrongly. A
        figure the engine actually counted is named at any size above empty; a **view** is never
        given one, since the rows belong to the tables under it.
      - **`SchemaStats` carries a lookup index and its `tables` are private**, because the badge
        lookup is per *row* of the schema tree and one landing invalidates every badge in the
        database at once: `iter().find` cost 4.2 ms at 2,000 tables, 24.8 ms at 5,000 and 95.9 ms at
        10,000, in a single frame on the UI thread (measured on the shipped core, release). Build one
        with `SchemaStats::new`; the field is private so a literal cannot fill the `Vec` and leave the
        index empty, which would answer *"no statistics for this table"* for every row, silently. The
        index keys on the **name** alone so a lookup borrows its `&str` instead of allocating a key,
        and the handful of same-named tables in other namespaces are separated inside the bucket.
      - The panel and the copied Markdown are two renderings of one claim, so each rule they share
        is one function: `RowCount::qualifier` (the word a figure is qualified with — the two had
        drifted to "(estimated)" against "(estimate)", with only the Markdown half tested),
        `TableStats::row_caption`, `TableStats::shows_free` (MySQL's `DATA_FREE`, and only when there
        is some), `index_facts` + `unused_note` (an index's own line), and `count_row_state` — whose
        `None` is the state that removes the **Count rows** row rather than leaving a blank band
        where the control was. `IndexStats::cardinality_label` is the one reader of
        `IndexStats::cardinality`: it is an InnoDB sample of ~20 index pages, and printed through the
        *exact* branch it read as a measurement — `3,996,120`, on the clipboard as well as in the
        panel, four lines under a row count carefully marked `~4.21m (estimated)`.
- `schemaic-db` — MySQL/MariaDB (`mysql_async`) + SSH tunnels (`ssh.rs`), PostgreSQL in `pg.rs`,
  SQLite in `sqlite.rs`, and
  the pinned manual-transaction connection in `session.rs`. **`Db::fetch_sessions`/
  `Db::kill_session`** are the Server Activity panel's whole backend, and they are three queries per
  engine rather than one: MySQL runs `information_schema.PROCESSLIST` (required — without it there
  is no panel) plus `INNODB_TRX` and a lock-wait join, both *best effort*, because those two are
  what need `PROCESS` privileges and what differ by server. The lock-wait join has no single
  spelling — MySQL 8 removed `information_schema.INNODB_LOCK_WAITS` and MariaDB has no
  `performance_schema.data_lock_waits` — so the pair is tried in turn, and when neither works the
  panel still knows *who* is blocked (`trx_state = 'LOCK WAIT'`) and simply cannot say by whom,
  which is the honest degradation. PostgreSQL needs one query: `pg_stat_activity` filtered to
  `backend_type = 'client backend'` (the checkpointer and the WAL writer are processes, not
  sessions, and would sort to the top forever) with `pg_blocking_pids(pid)` for the graph, which
  resolves the transitive case a hand-written `pg_locks` join gets wrong. A row whose `pid` won't
  parse is **dropped**, never admitted under id `0`: a session `0` renders a full row with a live
  "Kill session" under it and can be pointed at by other rows' edges, which is the same reasoning
  `parse_pid_array` already applies to the graph. **Both exclude the caller's own connection** —
  every operation here opens a fresh one, so the poller would otherwise report itself running the
  activity query at the top of every refresh. **Both also sort blocked-or-working sessions above
  idle ones before the `LIMIT`**, and that is not cosmetic: `activity::rank` puts lock waits at the
  top of the panel, but a session that started waiting four seconds ago has the *smallest* age on
  the server, so ordering the fetch by age alone — which reads like "keep the interesting end" —
  cut every row of a lock pile-up on a box holding three thousand idle pool connections and left a
  quiet-looking list during an incident. PostgreSQL sorts exactly (`blockers <> '{}'`, read out of a
  subquery so `pg_blocking_pids` is evaluated once per backend rather than again for the `ORDER BY`);
  MySQL settles for `COMMAND <> 'Sleep'`, because `PROCESSLIST` carries no lock information and the
  view that does needs the `PROCESS` privilege this required statement deliberately doesn't. Both
  methods gate on `activity::supports_activity` / `supports_kill` **before** the engine `match`: the
  `match` picks a query set, which no capability can paper over, but the predicate is what stops a
  fourth engine falling through the MySQL arm into `information_schema.PROCESSLIST` and failing
  three catalogue lookups deep. Kills are `KILL QUERY` /
  `KILL CONNECTION` and `pg_cancel_backend` / `pg_terminate_backend`, always on a new connection,
  since the session being killed may be the one holding everything else up. **`Session::server_id`
  exists for this panel**: MySQL's thread id comes back in the handshake and PostgreSQL's backend
  pid is asked for at open, and holding it is what lets the app recognise one of its own pinned
  Manual-mode connections in a list of session ids — see the repair below. SQLite errors rather
  than answering empty (`core::activity::supports_activity`); the app is gated not to ask.
  Populates each result column's
  `origin` (real table/column + key flags) from the wire protocol. Connection **identity** is the
  `Db` handle (`Db::connect(&Connection, tunnel_port)`), not a `mysql://…` URL — credentials go
  through `OptsBuilder` (passwords with `@ / # ? %` need no escaping; no plaintext URL anywhere).
  Schema introspection fills the **full** column model (see `core::schema`): MySQL from
  `information_schema.COLUMNS` (`COLUMN_DEFAULT`/`EXTRA`/`COLLATION_NAME`/`COLUMN_COMMENT`/
  `GENERATION_EXPRESSION`) + `STATISTICS` (`SUB_PART`/`COLLATION` for prefix + `DESC`); PostgreSQL
  from **`pg_catalog`, not `information_schema`** — `format_type(atttypid, atttypmod)` is the only
  source of the *declared* type (`udt_name` gives `varchar`, losing the `(45)`), plus
  `pg_get_expr` for defaults and `attidentity`/`attgenerated`. `mysql_column` normalizes the
  MySQL/MariaDB `COLUMN_DEFAULT` divergence (MariaDB returns SQL text, MySQL a raw value needing
  quoting) — pure + tested, since getting it wrong writes a *different* default rather than failing.
  MySQL additionally reads each table's engine/collation/comment, each FK's
  `REFERENTIAL_CONSTRAINTS` rules, and each view's `CHECK_OPTION`/`DEFINER`/`SECURITY_TYPE`
  (`mysql_view_options`; `ALGORITHM` too, but only on MariaDB — MySQL 8 has the column
  nowhere but `SHOW CREATE VIEW`, so the query holds its shape with a `CAST(NULL AS CHAR)`
  and `Db::view_algorithm` reads it **lazily, per view**, when the editor opens: one
  `SHOW CREATE VIEW` each is too many round-trips for a schema fetch, and the value is only
  needed for the view being redefined. `view_algorithm_of` is the pure reader — it takes the
  clause *positionally*, between `CREATE` and `DEFINER`, since everything past that is the
  user's own SQL and can say `ALGORITHM=` too);
  CHECK constraints come from `CHECK_CONSTRAINTS`, which the two servers shape differently
  (MariaDB has `TABLE_NAME`, MySQL 8 needs a `TABLE_CONSTRAINTS` join that is also the only
  home of `ENFORCED`) — and only `ER_UNKNOWN_TABLE`/`ER_NO_SUCH_TABLE` degrades to "no checks",
  for servers predating the feature, so a broken query surfaces instead of silently emptying
  every table's constraints. **`mysql_check_clause` is the third of these
  MySQL-vs-MariaDB text divergences** (with `mysql_column`'s defaults and the view
  `ALGORITHM`), and the rule is again "normalize on the way in": MySQL 8 returns
  `CHECK_CLAUSE` with one *extra* level of backslash escaping — `_latin1'new'` arrives
  as `_latin1\'new\'` — where MariaDB returns it byte-for-byte runnable. Restating
  MySQL's verbatim is a **syntax error**, and unescaping MariaDB's would eat the
  backslash out of `'it\'s'`, so it is gated on the `mariadb` flag and measured against
  `SHOW CREATE TABLE`. It also has to run *before* `ddl::check_predicate`, whose paren
  scan reads string boundaries that `\'new\'` doesn't have;
  **The standalone PG objects** come from `pg_types` (enums *and* domains in one `pg_type`
  scan — both live there, `typtype` `e`/`d`) and `pg_sequences`, each folded by a pure,
  tested half (`pg_fold_types`/`pg_sequence_row`). Four decisions are written down because
  each is a bug the live pass would otherwise have shipped: an enum's labels arrive **one
  row each** rather than string-aggregated, since a label is arbitrary text and every
  separator is a value some database already stores (`'a,b'`, an embedded newline, `''`);
  a domain's "has a default" is its own column, because `DEFAULT ''` read off the text
  would come back as *no* default and get dropped on every replay; a domain's collation is
  reported only when it differs from its base type's, or every `text` domain would open
  with a phantom `COLLATE`; and a sequence's definition is read from the `pg_sequence`
  **catalogue** while only `last_value` comes from the `pg_sequences` **view**, so a role
  that may see the schema but not the data gets a full definition and a blank position
  rather than a vanished sequence. These run inside `fetch_schema`, where one failure takes
  the whole database's schema with it, so they go through `query_all_optional` — which
  degrades on `undefined_table`/`column`/`function` only, the same two-sided rule
  `mysql_checks` follows.
  PG reads `confdeltype`/`confupdtype`, the `pg_constraint` name behind a PK/unique index,
  and its view bodies from **`pg_get_viewdef` over `pg_class`, not `information_schema.views`**
  (which hands back an empty definition to a non-owner and omits materialized views entirely)
  plus `reloptions` for the storage params a replace would reset (`pg_view_options`) — all
  folded on *after* the shared `assemble_schema` (which both engines share and neither's
  extras belong in).
  `fetch_query`/`run_batch`/`fetch_schema`/`ping`/`commit_writes`/`refetch_rows`/`prepare_check`
  (non-executing `PREPARE` for the editor's live validation)/`run_ddl`/`fetch_table_stats`/
  `count_rows` are `Db` methods taking the target DB per call.
  `fetch_table_stats` fills `core::stats::SchemaStats` for a whole database — one round trip
  either way, and having the set is what feeds the schema tree's size column. It is **lazy and
  deliberately not part of `fetch_schema`**: selecting `DATA_LENGTH` from
  `information_schema.TABLES` makes MySQL materialize per-table statistics, and the schema fetch
  runs on every connect. MySQL adds `STATISTICS` for cardinality and
  `performance_schema.table_io_waits_summary_by_index_usage` for index usage — that last one is
  routinely off or ungranted, and a failure there leaves every scan count `None`, because zero is
  what marks an index unused. PostgreSQL reads `pg_table_size`/`pg_indexes_size`, `reltuples`
  (`-1` is "don't know", so it becomes `None`), `n_dead_tup` and the last analyze, through
  `query_all_optional` so a restricted `pg_stat_*` can't fail the fetch; a **partitioned parent**
  is listed with null sizes rather than the truthful `0` that would read as an empty table.
  A partitioned **index** (`relkind = 'I'`) carries the same guard in its own spelling, and did not
  at first: `pg_relation_size` on one returns `0`, so an index spread over 40 GB of partitions
  printed `0 B` in the panel and exported it — the exact reading the sibling query's comment says it
  avoids. Its *scan* count is deliberately unguarded: `pg_stat_all_indexes` has no row for such an
  index, and NULL there is the truth ("nobody counted"), which is what stops `is_unused` flagging it.
  Both builders are string-tested beside `user_schema_filter_excludes_only_postgres_internals` —
  that convention exists precisely to catch a guard present in one query and absent from its sibling,
  which is what shipped. On the MySQL side the three queries are named constants and the
  rows-to-model step is a pure `map_mysql_stats`, so the decisions with a wrong answer that looks
  plausible — `MAX(CARDINALITY)` per index rather than per key *position*, the usage view's NULL
  index row, a missing usage entry leaving `scans: None` — are unit-tested without a server.
  SQLite returns an empty set — it publishes none (`stats::supports_table_stats`), and
  `count_rows` is not a fallback there but the only figure there is.
  **One fetch per database, shared by all three surfaces.** The properties modal used to issue its
  own, on its own connection, on every open — ten tables inspected in a row was ten whole-database
  catalogue fetches for figures already in memory, and worse on a server with
  `information_schema_stats_expiry = 0`, which re-reads them from the storage engine each time. It
  now reads `ConnNode::stats` when that slot holds figures for the active connection's database, and
  *warms* the slot when it does have to fetch, so the modal and the tree spare each other in both
  directions. Another connection's target still fetches on its own: `db_nodes` is the active
  connection's tree, which is why the target carries a `conn_id` at all.
  **`count_rows` takes a `CancellationToken` like every other unbounded operation.** It was the one
  that didn't, and the scan it starts is a full one: closing the modal abandoned the *answer* while
  the query ran on for minutes holding a connection, and reopening offered the button again, so N
  opens stacked N concurrent scans. Each engine cancels its own way — MySQL `KILL QUERY` from a
  second connection, PostgreSQL `cancel_token`, SQLite the interrupt handle, which is why its count
  no longer goes through `with_conn` (the handle has to reach the async side before the scan starts).
  The app holds one token beside `properties_counting`, cancels it whenever the modal's target
  changes (Escape, the ✕, the backdrop, reopening elsewhere) and offers **Cancel** beside the spinner;
  a cancelled count is not reported as a failure, since the estimate the user already had is what the
  panel goes back to. `run_ddl` is the schema-editing apply path and is **honest about
  atomicity**: PostgreSQL runs the whole plan in one transaction (transactional DDL), MySQL runs
  it sequentially and reports which statement failed *and how many already stuck*
  (`DdlError::applied`) — every MySQL DDL statement commits implicitly, so a transaction there
  would be theatre. **SQLite's DDL is transactional too**, so `sqlite::run_ddl` wraps the whole
  plan in one and rolls it back whole, which is why every `DdlError` from that backend carries
  `applied: 0` — a half-applied plan is a state this engine never leaves behind, so there is no
  partial progress for the report to admit to (`sqlite::ddl_tests`, over in-memory SQLite). That
  arm no longer refuses every plan: the gate moved upstream to `ddl::supports_change`, which can
  see the `Change` where `run_ddl` has only strings, and refusing wholesale here would have taken
  away the drops SQLite genuinely has.
  **It also suspends foreign keys for the duration and checks them before the commit** — `PRAGMA
  foreign_keys = OFF` outside the transaction, since SQLite ignores the pragma inside one. It used
  to turn them *on*, and enforcing during a plan is not the safe reading it looks like: with them
  on, a rebuild's `DROP TABLE` on a parent is an implicit `DELETE FROM parent`, which fires
  `ON DELETE CASCADE` and empties the child tables — the table comes back exactly as the user drew
  it and another one has quietly lost every row (`sqlite::rebuild_fk_tests` rebuilds a one-row
  `artist` and watches both `album` rows behind an `ON DELETE CASCADE` vanish). That is why step 1
  of SQLite's own twelve-step procedure turns them off. (The rebuild's statement list carries its
  own `ddl::FK_OFF`/`FK_ON` pair for the *other* consumer — see the twelve-step rebuild above; both
  are no-ops inside this transaction, which is exactly why this one has to stay.) Nothing is given
  up by it: `PRAGMA foreign_key_check` runs against the *finished* state before the commit and
  refuses the plan if a reference dangles, naming the child table and the parent so the refusal is
  actionable — a stricter question than the per-statement one, since a plan is allowed to pass
  through states no single statement could.
  **What it must not refuse is what the file arrived with.** That pragma scans the whole database,
  and a `.db` written by the sqlite3 CLI — where foreign keys are off by default — very commonly
  carries a child row whose parent is gone. Read as the plan's doing, it made adding a column to an
  unrelated third table fail with *"the plan leaves a foreign key pointing at nothing"*, and every
  DDL operation on that file fail the same way for ever. So the violations are read **before**
  `BEGIN` as well, identified by `(table, rowid, parent, fkid)`, and only what
  `FkViolations::added_since` reports as new refuses the plan (capped at 10,000 rows, past which it
  falls back to comparing counts).
  **Step 9 is asked here too, and of the engine.** SQLite's own procedure says to deal with the views
  a schema change affects, and a plan has no way to: `legacy_alter_table = ON` — right for the
  rename — is exactly what stops the engine noticing, so a rebuild that dropped or renamed a column
  a view names *reported success* and the user found out the next time they opened the view. (The
  **native** `ALTER TABLE … DROP COLUMN` is refused by the engine for the same case, so this was a
  divergence inside one feature.) The authority on what a view resolves to is SQLite, so
  `broken_views` asks it: for every view in the catalogue, *prepare* `SELECT * FROM <view>` — a parse
  against the schema the plan left behind, no rows read. Read before `BEGIN` as well and compared
  (`first_newly_broken_view`), on the same inherited-vs-added reading as the foreign keys: a `.db`
  can arrive with a view over a table that is already gone, and refusing for that would take DDL away
  from every other table in the file. A plan that broke one is rolled back whole, naming the view and
  the engine's own reason (`sqlite::rebuild_bystander_tests`).
  **`Db::trigger_source` is the fourth MySQL-vs-MariaDB text divergence** (with `mysql_column`'s
  defaults, the view `ALGORITHM` and `mysql_check_clause`) and the only one that is not an
  optimisation: `information_schema.TRIGGERS.ACTION_STATEMENT` on MySQL 8 returns the body with
  its escapes **already resolved** (`'C:\temp'` → `C:`,0x09,`emp`; `'it''s'` → `'it's'`), and the
  damage is *not* recoverable by re-escaping — so a recreate writes a different trigger, or fails
  1064 after the `DROP` has committed and destroys it. `SHOW CREATE TRIGGER` is the only faithful
  source and carries `sql_mode`/`character_set_client`/`collation_connection` in the same row;
  `trigger_body_of` is the pure positional reader beside `view_algorithm_of` (anchored on the
  first `FOR EACH ROW` at a *code* position, since a table can be named `` `x FOR EACH ROW y` ``).
  Read **lazily, per trigger**, when the editor opens. MariaDB returns everything verbatim.
  `mysql_triggers` also gives a group's *leading* trigger a `PRECEDES` anchor: MySQL appends a
  no-clause `CREATE TRIGGER` **last**, so replacing the leader silently reversed the firing order.
  On the PG side, both of `pg::fetch_schema`'s trigger queries filter `tgparentid = 0` inline (a
  partition's cloned trigger is `tgisinternal = false` and can only be dropped through its parent),
  and `UPDATE OF` columns + a function's `proconfig` arrive **one row each** rather than
  string-aggregated — same rule the enum labels follow, and for the same reason.
  **Stored routines** are read with the rest of the schema on both engines that have them.
  PG has **three** filters over one reader (`routines_where`), and which one a caller stands on is
  the whole design. `routine_scope` is the floor both share: `prokind IN ('f','p')` (an aggregate or
  a window function has no body to show and no `CREATE FUNCTION` that would recreate it; the column
  is PG 11+, and there is no pre-11 spelling that tells a procedure from an aggregate) in a user
  namespace. `routine_filter` adds **not owned by an extension** (`pg_depend.deptype = 'e'`) and is
  what the schema tree browses — that exclusion is applied here and not to the types beside it, and
  the difference is degree rather than principle: an extension installs a handful of types and
  hundreds of functions, and PostGIS alone would bury a database's own routines under ~1000 `st_*`
  rows in whichever namespace it was created in. **`trigger_function_filter` does not add it**, and
  that is not an oversight: a trigger binds to whatever returns `trigger`, extension-owned or not
  — `moddatetime` is the standard "touch the modified column" function and arrives exactly that
  way — and the picker reading it is a dropdown with no free-text entry, so a function missing from
  that list is a function no trigger can be pointed at. It narrows on `prorettype` instead, on the
  **server**: filtering in Rust meant shipping every routine body in the database over the wire on
  a call the trigger editor makes every time the routine editor closes back to it. Both queries
  inside `routines_where` take the same filter string, because the settings fold is keyed on oid
  and a filter that disagreed would attach one routine's `SET` clauses to nothing.
  MySQL reads `information_schema.ROUTINES` plus `PARAMETERS` — that server publishes no rendered
  signature, so `mysql_routines` rebuilds the `IN a INT, OUT b TEXT` form from one row per
  parameter, keyed by name **and kind** because a function and a procedure may share a name, and
  excluding `ORDINAL_POSITION = 0`, which is a *function's return value* rather than a parameter.
  **The `ROUTINES` read binds by the query's own aliases, not by position.** The query is the
  `MY_ROUTINES_SQL` const so a test can read it, and `my_routine_row` → `my_routine_row_from` maps
  `n`/`ty`/`body`/`sqlmode`/… onto `MyRoutineRow`'s fields. It is a struct rather than the tuple
  its siblings here are because fourteen columns is past `mysql_common`'s twelve-element `FromRow`
  ceiling — which is the same thing as saying the compiler stopped checking the arity — and the
  reader that replaced the tuple indexed `0..=13` against a `SELECT` fifteen hundred lines away
  with nothing but a doc comment holding the two in step: insert a column at position 3 and `body`
  starts reading `CHARACTER_SET_NAME`, `sql_mode` starts reading `ROUTINE_COMMENT`, the suite stays
  green, and what ships is a routine whose Body field shows `utf8mb3` and a recreate built from
  that — on the engine whose `DROP` commits on its own. `my_routine_row_from` takes the reader as a
  closure so a test can supply one (`mysql_async` doesn't re-export `mysql_common`'s row
  constructor), and the test-only `MY_ROUTINE_COLUMNS` is a **third** statement of the list on
  purpose: the two that matter are the query and the reader, and checking one against the other
  would only be checking whether the same hand wrote both.
  `Db::routine_source` is the fifth text divergence and the counterpart of `Db::trigger_source`:
  `ROUTINE_DEFINITION` resolves the body's escapes exactly as `ACTION_STATEMENT` does, and every
  MySQL routine edit begins with a `DROP` that commits on its own, so `SHOW CREATE` is the only
  source an edit may be built from. `routine_body_of` is its pure reader; unlike `trigger_body_of`
  it has no keyword to anchor on — a routine has no `FOR EACH ROW` — so it skips the parameter list
  as a balanced group and then consumes characteristics by keyword until one isn't. Consuming
  greedily is safe because the two vocabularies are disjoint: no MySQL statement begins with
  `COMMENT`, `LANGUAGE`, `NOT`, `DETERMINISTIC`, `CONTAINS`, `NO`, `READS`, `MODIFIES`, `SQL`,
  `DATA`, `SECURITY`, `DEFINER`, `INVOKER` or `RETURNS`. **A `RETURNS` type's modifiers each take
  their argument with them, or take none** — `TYPE_FLAG` for `UNSIGNED`/`ZEROFILL`/…, `TYPE_NAMED`
  for `CHARSET x`/`COLLATE x`, and `CHARACTER SET x` matched as the three-word form it is. A
  keyword consumed without its value leaves that value at the head of what is returned as the body,
  which is the worst failure this reader has: the editor shows a corrupt body and the Apply emits a
  1064 *after* the `DROP` has committed. A bare `SET` is on neither list for the same reason from
  the other side — it swallowed the first word of a body that legitimately begins `SET @x = 1`.
  Both the `Create` column and
  `ROUTINE_DEFINITION` are **nullable** — MySQL returns NULL to an account without `SHOW_ROUTINE`
  or ownership — and an unread body leaves the editor on whatever the schema fetch carried rather
  than blanking it, while the row's session values are returned either way (see `RoutineSource`).
  SQLite has neither call: it has no stored routines at all.
  **`sqlite.rs` is the third engine**, and five things make it unlike the other two rather than a
  third set of catalogue queries. **There is no server**: a connection is a *file*
  (`Connection::file`), so host/port/user/password/SSH are all inert, `fetch_databases` has nothing
  to enumerate (it reports the one database SQLite calls `main`), and `Db::connect` refuses to let
  a tunnel port repoint the file. **It still opens the file to answer**, through `ping` — the schema
  sidebar empties the tree when a listing fails, which the other two engines get for free because
  their listing is a query; answering `main` without touching the file gave a missing or locked one
  a node in the tree whose every child fetch then printed the connect error *inside* it, under a
  database that isn't there and beside a header already saying "Disconnected". **"Inert" has to be enforced twice, because the engine picker is
  editable on a saved connection and the SQLite form renders no SSH block** — so a connection
  switched over from a tunnelled MySQL one kept `ssh.enabled` set with no control anywhere that
  could unset it, and every operation on a local file dialled a bastion with a stored credential and
  failed outright when that host was down. `Connection::uses_tunnel()` is the one answer every
  tunnel site asks (there are six), and `Connection::sanitized()` — which `DraftSignals::to_connection`
  returns through — drops the server side on save so the state cannot exist. `is_networked` likewise
  has a single definition in `core::connection`, which `db::Engine` and the form's `DbKind` both
  delegate to; it had two, and the third consumer not asking at all is what this cost. **The driver is blocking**, so every call runs in
  `spawn_blocking` and opens its own connection there — which is not a compromise but exactly the
  one-connection-per-operation invariant, at microsecond cost on a local file; cancellation goes
  through `Connection::get_interrupt_handle`, the analogue of `KILL QUERY` that needs no second
  connection. **Values are dynamically typed**: a declared type is an *affinity*, so `value_of`
  reads the storage class of the value in front of it rather than trusting the column, and a BLOB
  renders as its size, having no lossless text form. **Column provenance is not available from the
  driver at all** — SQLite's C API has `sqlite3_column_table_name`, but only under
  `SQLITE_ENABLE_COLUMN_METADATA`, and rusqlite exposes neither the flag nor the call (measured
  against 0.32.1 and 0.40: there is no `column_metadata` feature and `libsqlite3-sys` generates no
  binding), so a result's `origin` is derived from the *statement* instead and anything but a
  plainly single-table `SELECT` is left `None`, which the editing system already reads as
  not-editable. **Every rowid table has a key, and it isn't a column.** A table with no primary key
  and no usable unique index is read-only on the other two engines because there is genuinely no
  way to name one of its rows; on SQLite there always is one, unless the table was declared
  `WITHOUT ROWID`. Such a table is therefore opened as `SELECT rowid, * FROM t`
  (`filter::table_query`) and its leading column marked `ColumnOrigin::implicit_key`, which is what
  `edit::resolve_key` falls back to. Three details there are load-bearing. `has_rowid` asks
  **`PRAGMA table_list`'s `wr` column** rather than searching the stored `CREATE` text, which a
  comment, a newline, a trailing `, STRICT` or a column named `without_rowid` each defeat; a view or
  a name that isn't there answers `false`. `implicit_row_key` **chooses** the spelling from
  `ROWID_ALIASES = ["rowid", "_rowid_", "oid"]` instead of using the constant `"rowid"`, because
  SQLite lets a table declare a column called `rowid` and then *that* column is what the word means:
  the first unshadowed alias wins, case-insensitively, and a table that has taken all three has no
  way left to name its rowid and stays read-only. And `attach_origins` looks a declared column up
  **first**, synthesising an implicit-key origin (`not_null`, `auto_increment`, not `primary_key`)
  only for an unshadowed alias on a table that has a rowid, recording the name **as written** so the
  `WHERE` the write-back builds resolves to the same value the `SELECT` read. `GridWrite::plan`, the
  delete → update → insert order, the 1-row safety net and `one_row_verdict` are untouched — the key
  simply names a column the table doesn't have, and SQLite resolves `rowid` in a projection and in a
  `WHERE` alike, which is also why the in-place splice re-fetch works
  (`a_keyless_table_writes_back_through_its_rowid`,
  `a_refetch_reads_a_keyless_row_back_by_its_rowid`).
  **But a rowid is not a row identity, and the safety net alone cannot see that.** SQLite reassigns
  them: the twelve-step rebuild used to renumber a keyless table, an insert after a delete takes the
  freed number, `VACUUM` compacts them — and nothing re-runs an open result tab when any of that
  happens, so the grid can hold a number that now names a *different* row. Keyed on the number
  alone the `UPDATE` lands on that row and affects exactly 1, which is the number `one_row_verdict`
  is looking for; the net's whole premise is that a stale key matches **zero**. Two things restore
  it. `EditTable::confirm_cols` carries the values the grid read into the same `WHERE` — the rowid
  still does the identifying and the values only *confirm* it, which is why this is not the
  match-on-all-values scheme a keyless table makes unsafe (duplicate rows are legal there, and the
  rowid tells them apart). It is populated only for an implicit key, excludes binary columns whose
  cell is a placeholder rather than a value, and `edit::row_key` is the one builder that appends it,
  so update, delete and the row panel's immediate save cannot disagree about what a row is. And
  `sqlite_rebuild_sql`'s copy now names `rowid` explicitly — gated on `TableInfo::implicit_key`
  being reachable and on no draft column shadowing any of the three spellings — which stops the
  renumbering at source and preserves the gaps a delete left. A `WITHOUT ROWID` table is unchanged in every
  respect — SQLite will not even prepare the statement against one — and a view gets
  `implicit_key: None`. Verified end to end against a copy of the EdgeCases file: `keyless` opens as
  `SELECT rowid, * FROM keyless ORDER BY rowid ASC LIMIT 100`, reports `editable = [false, true,
  true]`, and an `UPDATE` plus a `DELETE` committed through it affected exactly 2 rows, while a bare
  `SELECT * FROM keyless` stays read-only. **Showing the rowid is the trade, and it was the
  argued-for one.** Hiding it would mean a result column that isn't a column, which `ResultSet` has
  no notion of, so export, copy, aggregates, virtualization, widths, the header filter and the row
  panel would each have to learn to skip it and every site that forgot would leak it — and the grid
  would stop agreeing with the SQL in the editor. Re-fetching the rowid per row at write time is
  circular: identifying the row is the problem, and matching on all non-key values is exactly what a
  keyless table makes unsafe, duplicate rows being legal there. The cost is one extra column; the
  gain is no new concept in `ResultSet` or the grid, the reason a table is editable staying legible
  in the statement, and a hand-typed `SELECT rowid, * FROM t` that is editable with no
  "this came from the tree" plumbing.
  Introspection is `sqlite_master` plus the pragmas, and three of its decisions are
  the sort that only show up against a real database: `table_xinfo` rather than `table_info`,
  because only the former reports a **generated** column and a write path that can't see one offers
  to insert into it; a non-`INTEGER` `PRIMARY KEY` is reported **nullable**, because SQLite
  documents that it really is; and an index whose predicate or expression key the pragmas don't
  return is marked `lossy`, since an index edit is a drop-and-create and would otherwise silently
  widen it. That last one is also why every index carries `IndexInfo::create_sql`, the statement
  that declared it — `sqlite::index_sql` is one `sqlite_master` query behind two consumers, Copy
  DDL's appended `CREATE INDEX` block and the per-index text a rebuild replays instead of
  re-emitting a `lossy` index from the model. It is `None` for an index SQLite wrote itself to back
  a `UNIQUE` or `PRIMARY KEY`, whose `sql` is NULL because it is part of the table's declaration.
  A fourth is a **non**-finding worth recording, because it looks like a gap: a table
  whose primary key is an `INTEGER PRIMARY KEY` reports **no indexes at all**, since that column
  *is* the rowid and SQLite builds no separate index for it. Nothing needs to be synthesised —
  `edit::resolve_key` reads the primary key off `ColumnInfo::primary_key`, not off the index list,
  so write-back keys on it correctly (verified end to end against a real file); inventing a
  `PRIMARY` index entry would only put an object in the tree that the database doesn't have. That
  is not in tension with the implicit key above, which *is* synthesised: there the table declares no
  key at all and the rowid is the only identity it has, whereas here the table already has a key and
  it is a real column — the rowid under another name.
  **CHECK constraints have no pragma at all**, and `fetch_schema` used to hard-code
  `check_constraints` empty — harmless until a rebuild could write the table from the draft, where a
  check missing from the draft is a check the rebuild silently drops. `checks_of` reads them off the
  table's own `CREATE` text instead, the position `generated_expr_of` is already in and with the
  same tools: the shared boundary lexer, so a column called `check_sum` or the word inside a string
  or a comment can't match, and `core::sql::balanced_paren_span` for the predicate, which may
  perfectly well hold a comma or a `')'` inside a literal. A column-level and a table-level check
  read alike, because SQLite makes no distinction once the table exists and a rebuild restates both
  as table constraints; a constraint written without `CONSTRAINT <name>` keeps an **empty** name,
  which is the honest answer rather than a gap (`sqlite::check_text_tests`/`check_schema_tests`).
  **One catalogue read of the trigger text feeds two consumers** — `trigger_sql` is the query, and
  the two things made from it are not redundant (see `TableInfo::dependent_ddl` above).
  `trigger_statements` re-terminates each one into `dependent_ddl`, because SQLite stores a
  statement without its `;` and a trigger body is full of internal ones, so a replay would
  otherwise run into whatever follows it. `triggers_of` parses the *same* text into `TriggerInfo`
  through `ddl::sqlite_trigger_info`, this being the one engine where introspecting a trigger means
  reading SQL; a statement the parse can't read is left out rather than guessed at, the direction
  `view_body_of` already refuses in. `fetch_schema` fills `triggers` for a **view** as well as a
  table — an `INSTEAD OF` trigger is the only way a SQLite view is written to — while
  `dependent_ddl` stays a table's business, nothing rebuilding a view.
  A view now also gets `view_options: Some(…)` rather than `None`. SQLite has none of the options
  the other two carry — no definer, security type, algorithm, storage parameters or check option —
  and exactly one the re-create behind every view edit would otherwise drop: the explicit column
  list, read by `view_columns_of`, a third positional reader over the shared lexer beside
  `view_body_of` and `checks_of`. It is the one parenthesised group *before* the header's `AS` at
  code position and paren depth zero, both qualifications carrying the weight they do in
  `view_body_of` — after that `AS` a `(` is the user's own arithmetic or a subquery.
  `EXPLAIN QUERY PLAN` is what `explain` runs — plain `EXPLAIN` disassembles to VDBE
  opcodes, which is a different artefact and useless to `core::plan` — and there is no analyzing
  form, since SQLite will not execute a statement to time it. **Manual transaction mode is refused**
  (`Session::open`): a pinned `rusqlite::Connection` is blocking and `!Sync`, so holding one across
  awaits means a dedicated thread and a channel, which is worth building deliberately rather than as
  a side effect — and silently running the tab's statements on fresh connections would break exactly
  the promise the mode makes. That refusal is the one thing in `session.rs` a test can reach, and it
  is now tested twice: that a SQLite `Db` gets `DbError::Connect` rather than a session, and that it
  gets it **before** anything is opened (a refusal arriving after a connect attempt would surface as
  a file error instead of the explanation, and on a real path would create the file). The engine →
  `tx::TxEngine` mapping moved out of the `Session` method into a free `tx_engine_of` for the same
  reason — a `Session` cannot exist without a live connection, and reading MySQL's forgiving model
  as PostgreSQL's poisoned one is the difference between "still committable" and "discard
  everything".
  SSH tunnels return a `TunnelHandle` (drop → port freed) with
  keepalives + TOFU host-key verification (`ssh_known_hosts.json`).
  `import_rows` is the bulk-load path (both engines): one transaction of batched multi-row
  `INSERT`s pulled from a `RowSource` iterator, each batch required to affect exactly as many rows
  as it carried — the `commit_writes` 1-row safety net scaled to a file, without its
  statement-per-row round-trips.
- `schemaic-ai` — persistent `claude` CLI session (stream-json), turn parsing.
- `schemaic-term` — terminal panel + shell (`shell.rs`).
- `schemaic-ui` — the Floem UI. The central `Ui` struct (threaded everywhere) is split per-domain:
  `Copy` signal bundles (`TabsUi`/`SchemaUi`/`ConnUi`/`AiUi`/`TermUi`/`LayoutUi`/`OverlayUi`) +
  `Rc<…Actions>` callback bundles — so `ui.run` is `ui.tab_actions.run`, `ui.db_nodes` is
  `ui.schema.db_nodes`, the tabs signal is `ui.tabs_ui.tabs`. Modules:
  - `consts.rs` — layout/dimension constants + `MONO_FAMILY` (glob-imported). Any SQL/code
    surface reads that one name — the diff view, and `FieldCfg::mono` (the DDL preview's
    script box, the view editor's definition). `FIELD_INPUT_H` is the same idea for the
    **compact single-line field** every transient bar wears (the editor's find/replace/goto, the
    grid's find/goto, the row panel's inputs): `FieldCfg::height` is an `Option`, and leaving it
    off is not a neutral default but a *different* control — `None` derives the box from content
    at `line_h + CHAT_PAD_V * 2 + 3`, which is 34px against this 26. The grid's find bar shipped
    without it and stood 8px taller than the identical editor bar beside it, so a bar that means
    to be compact says so with this constant and never with a literal.
  - `widgets.rs` — reusable widgets: `menu_panel`/`MenuEntry`, `modal_title`/`panel_style`/
    `menu_item_style`, `window_size`, `autohide`/`shift_hscroll`/`wheel_hscroll` scroll wrappers,
    `section_title`/`centered_msg`/`toggle_icon` (and `toggle_icon_gated`, whose panel may not be
    available at all — the Activity toggle on a SQLite connection, dimmed and inert rather than
    opening an explanation), `measure_text_px`, `jump_to_bottom_button`. Also `MenuId`/`MenuFlags` —
    the single list of the app's mutually-exclusive dropdowns, which every trigger closes the others
    through (*Popup menus*).
    Also `focus_root` — the **one** way an overlay claims the keyboard (see the Floem gotcha on
    directed key dispatch): it takes focus on build *and* registers the view so Escape in a
    text field inside it hands focus back rather than dead-ending. Never spell out
    `.keyboard_navigable().request_focus(|| {})` again; that pair is what left every modal
    unclosable from the keyboard while a field was focused.
    Also the **shared modal form chrome** every modal wears — `form_setting`/`form_section`/
    `form_separator`/`FORM_GAP`/`control_button`/`footer_button`/`modal_footer`. Manage
    Connections set that shape and Import followed it; a new modal builds on these rather
    than copying them a third time.
    And the **keyboard-navigation cluster**, which is the subject of the Tab gotchas below:
    `FocusRing` (a modal's Tab order — `register`/`unregister`/`step_from`/`remember`/`focus_at`,
    plus the `ring_step` wrap rule and its deliberate opposite `list_step`, which clamps),
    `focus_root_with_ring`/`innermost_ring_root` (how the modal root and the *window* root enter
    it), `in_focus_ring`/`in_focus_ring_with` (how a non-field control joins, the second for one
    with teardown of its own — floem keeps a single cleanup slot), `VALUE_TAB` (where a growing
    block of stops starts), and the `PopupToken`-tagged `set_open_popup`/`clear_open_popup`/
    `dismiss_open_popup` slot. A field joins through `FieldCfg::focus` instead, since nothing
    outside floem's editor can see a key it has.
  - `markdown.rs` — AI-chat `render_markdown`/`CodeActions`/`code_block` (pulldown-cmark). A code
    block carries a **standing** 24px header — the language on the left, `Copy` and (for SQL only)
    `Insert` / `Run` on the right, as words rather than icons, since a permanent icon row over every
    block is noise. The header repeats the block's own `border_radius`: floem does not clip a child
    to a rounded parent, so a square-cornered fill would paint over the wrapper's arc. Also
    `proposal_card`: a fenced block tagged `core::propose::FENCE_TAG` renders as the AI's **proposed
    table change** — the table, the model's own summary line, the change count, and a Review button
    that hands the parsed `Proposal` to `CodeActions::propose`. The card is the offer, the DDL
    preview is the decision: Review runs nothing, it opens the same modal the designer opens.
    Recognition lives here rather than in a scanner of its own because pulldown-cmark is already
    parsing the reply and hands over the language tag; `core::propose` owns only the tag
    (`is_proposal_tag`) and the JSON step (`parse`), so there is one answer to "is this a proposal".
    **Gated on `settled`** (the turn has stopped streaming): pulldown-cmark closes an unterminated
    fence at end of input, so mid-stream a proposal would render as a card of half-arrived JSON,
    flickering "couldn't read this" on every chunk — until then it is an ordinary code block. A
    block that doesn't parse is neither dropped nor hidden: the user is told it couldn't be read
    *and* still sees what the model wrote, which is what they need to tell it what went wrong.
    **Which blocks get a Run button is `code_is_sql`, and it is the only place a model's output
    becomes one click from the user's database** — so the tag is authoritative (a ```bash block
    holding `DROP TABLE` is not SQL, however it reads) and only an *untagged* block falls back to
    `sql_leading_keyword`, a whole-word match on the block's first word. The fallback is
    deliberately narrow: a leading comment, an opening parenthesis or an empty block all read as
    not-SQL, because an untagged block is the uncommon case and a missing Run button is the safe
    way to be wrong. Both are unit-tested, including every keyword the list claims — one quietly
    dropped is a Run button that stops appearing with nothing else to notice it.
  - `settings.rs` — the three settings modals **and the four shared controls every modal's form is
    built from**: `focusable_toggle`/`focusable_toggle_row` (the switch — Space is ours, Enter is
    floem's), `focusable_dropdown` and the picker-agnostic `in_ring_dropdown` under it (which owns
    the four floem work-arounds a keyboard-operable dropdown needs). `themed_toggle` and
    `settings_dropdown` are the un-ringed builders beneath, and are **private** on purpose: a
    control nobody can Tab to is one left out of the modal's keyboard order by accident.
    `log_row` is the General section's one non-toggle: it names the log's **full path** (`log_hint`,
    which is the pure half and is tested) and reveals the folder holding it through
    `Ui::open_config_dir`. The log had been written and rotated since `logging.rs` landed, and
    locating it was still the user's problem — the one artefact a crash report needs sat at a path
    the app never said out loud, which the panic hook made worse by putting more in it. The
    *directory*, not the file: `schemaic.log` has no natural handler on Windows, `schemaic.log.1`
    is a second file worth reaching, and the same folder holds `tabs.json` and `connections.json`.
    Spawning a file manager is a process launch, so it is the app boundary's `open_config_dir` and
    not a `Command` in a view; a machine with no config directory gets the path-less hint and a
    disabled button rather than a control that silently does nothing.
  - `shortcuts.rs` — the app's keyboard shortcuts as **one table** (`SHORTCUTS`), which
    `settings::help_overlay` renders straight from — plus the tests that keep it honest. This list
    is the app's *only* keyboard documentation and for Ctrl+H / Ctrl+G the only affordance of any
    kind, so a binding missing from it is a feature nobody can find; it was a literal inside the
    modal and drifted exactly as its own comment predicted, hiding Alt+↑/↓, Ctrl+↑/↓, Ctrl+Shift+C/V
    and Ctrl+Home/End. **The handlers can't render from the table** — they are `match` arms on
    `Key::Character` spread across the files `KEY_FILES` names — so the guarantee runs the other
    way: the tests scan those files for the **five** idioms the codebase binds a
    **Ctrl/Alt + letter** with (the `"x" | "X"` case pair, `eq_ignore_ascii_case`, `NavKeys`'
    `Some("x") =>` *and* its `ch == Some("x")`, and `KeyCode::KeyX` for the physical match
    Ctrl+Alt+L needs) and fail when one has no row, with `EXEMPT` the justified-baseline
    escape hatch in the spirit of `contrast::UI_SHORTFALL`. The equality form is where **both**
    Ctrl+Shift+letter bindings live and was missed; the gate was green only because `p`/`t` are
    bound a second time in the unshifted arms. `KEY_FILES` pairs each file with the `SHORTCUTS`
    **groups** its bindings may be documented in, because a table-wide lookup let the editor's
    Ctrl+D vouch for a grid Ctrl+D — a different key doing a different thing. Deliberately
    **weak**, like `doc_coverage`: it catches the binding nobody wrote down, not an inaccurate
    row, and a *named* modified key (`Ctrl+Tab`, `Ctrl+Enter`, Ctrl+1‑9) is matched as a
    `NamedKey` and so is outside the scan entirely. Two rules
    earned their own tests — the scan skips `#[cfg(test)]` modules (a grid fixture's
    `Some("b".to_string())` was reported as a phantom Ctrl+B, and a gate that cries wolf gets
    deleted) and each idiom is pinned against synthetic input, since a scan that silently stops
    matching still passes a test that looks for what's missing. Only **modified letters** are
    gated: plain keys are bound in dozens of places for ordinary navigation, so gating them is all
    noise, and they're what a user tries anyway.
    `COMMAND_KEYS`/`command_keys` is the second consumer: the **command palette** shows a row's
    binding as a keycap at its far right, which is where someone who can't remember a key actually
    looks. Only where the command does *the same thing* the key does — a narrower set than it looks,
    since `Run` runs all statements while Ctrl+Enter runs the one under the caret, `Terminal` and
    `Ask AI` act on an argument where the keys only toggle a panel, and `Toggle Panel` names its
    panel as an argument so it has three bindings and therefore none. A nearly-right keycap is worse
    than none: it teaches a key that does something else. Each entry carries its row's **group**
    as well as the key string, which must be **byte-identical** to a `SHORTCUTS` row in that group
    (tested) — the table has two `Ctrl+G` rows meaning different things, so a string match alone
    let either vouch for the keycap; the
    other half — that each name still names a live command — can only be checked against a built
    registry, so it rides `overlays::assert_names_match_labels`' `debug_assert`, without which a
    renamed command would drop its keycap in silence.
  - `connection_form.rs` — Manage Connections modal + password-mask (+ tests). The form is built
    **once per open** while the list on its left keeps loading a different connection into the same
    `DraftSignals` — so every control the form owns a *separate* signal for has to be synced back
    down from the draft, not merely seeded from it. The password fields' `mirror_real` does that;
    the **Type** picker's `DbKind` did not, and reported the engine of whichever connection was
    active when the modal opened for every row the user then clicked — including which half of the
    form (`server_fields` vs `sqlite_fields`) got built, so a MySQL connection could be shown a
    *Database file* field and no host. Both directions now have an effect, and the write-back one
    tells a **pick** from a **load** by asking `connection::same_engine` whether the stored label
    still names the previous engine: on a pick it does, on a load it already names the new one.
    That is also what stopped opening the form from rewriting a `MariaDB` label to `MySQL` — the
    write used to be unconditional, and the picker has one name per engine where `db_type` has
    several.
  - `diff_view.rs` — Ctrl+K diff preview. `history_panel.rs` — Query History right-column panel.
  - `activity_panel.rs` — the **Server Activity** right-column panel (`RightPanel::Activity`, the
    footer's pulse-line toggle): the sessions on the active *connection's* server, a counts line, a
    lock-wait banner and a search box, over the same chrome as the History panel. It paints
    `core::activity` and decides nothing itself. Three things are worth knowing. The counts and the
    banner read the **whole** snapshot while the list reads the filtered one — they are facts about
    the server, and narrowing the list on screen must not make a blocked session vanish from the
    tally; the list then draws only `render_slice`'s front `RENDER_CAP` of that, since this whole
    container is rebuilt on every poll and five hundred rows of teardown and text shaping every two
    seconds is continuous churn in a panel whose subject is load. A refused kill lands in
    `ActivityUi::kill_error` and prints **above** the list rather than replacing it: routing it
    through `ActivityState::Failed` threw away the snapshot someone was reading mid-incident, banner
    included, and with auto-refresh Off nothing brought it back. `Failed` means "there is no
    snapshot"; a kill the server declined leaves the snapshot perfectly good.
    **Left-click is inert on purpose**: everything a row offers ends someone else's work, so
    both kills live in the right-click menu (with *Cancel query* dimmed rather than absent on a
    session with nothing running, so the menu keeps one shape), and the confirm is raised by the
    *app*, not here, so no route to a kill can skip it. The clock in the title bar wears the same grey as
    the refresh icon beside it: it was tinted while polling, on the reasoning that "Off" and "every
    2s" look identical between ticks, but two icons a few pixels apart in different colours read as
    one of them being *active* in the toggle sense. The interval is stated where a state belongs —
    the menu marks the chosen row, and the tooltip says it in words.
    Its dropdown is `overlays::activity_menu_overlay` and **not** a `popup_menu`: the panel is
    clipped for its collapse animation, so a menu built inside it is cut off at the panel's own
    edge, and a cursor-anchored popup wanders. It follows the schema tree's eye/gear machinery — an
    open flag plus an anchor the icon publishes from `on_move`/`on_resize` — with the one difference
    that makes it not a copy: the schema panel is against the window's *left* edge, so its menus can
    hang leftward and never meet one, while this panel is against the right. The anchor is therefore
    the icon's bottom-**right** corner, the menu is right-aligned to it at a fixed width, and the
    result is clamped into the window on both sides. That clamp **is** the edge detection.
    All three anchored dropdowns — the schema eye, the schema gear and this one — drop by one shared
    `MENU_ICON_DROP`, measured from the icon's **padded box** rather than from the glyph in it: the
    gear fills its box where the eye does not, and those two have always sat level, so the box is
    the reference and per-icon tuning by eye is the mistake it looks like a fix for.
    The clock's `.style()` is applied **before** its `.tooltip()`, which is not tidiness: `.tooltip()`
    wraps the view in a new one, so a style after it lands on the *wrapper* while the
    `on_move`/`on_resize` that publish the anchor stay on the container inside — which then reports a
    bare 16px glyph box with none of the padding around it, and the menu hangs under the glyph
    instead of under the control.
    Two layout facts are written into the rows because getting either wrong is invisible until
    someone puts a ruler on it. **A text view sizes its own taffy node to the text it measured**, so
    `width_full()` on the statement preview is not decoration — without something to be 100% *of* it
    lays out as one long line and the clip merely cuts it off; the width is what it wraps against.
    And **the age is pinned by `justify_between` on a two-child heading**, not by a `flex_grow`
    spacer before it: `space-between` says "the last child's right edge is the content box's"
    directly, where a spacer only arrives there if every base size in the row measured as expected —
    and a rich text measures ~20px wider than its glyphs, which is exactly how much short of its
    padding the age sat through two attempts to correct it from the outside. The identity group and
    the account name inside it both **grow**, which is the other half of the same lesson: nesting
    the name a level deeper made taffy measure it at min-content on some pass, and a rich text
    re-wraps to whatever width it is handed and then reports the *wrapped* size — so the next pass
    hands it the same small width and it stays wrapped. `schemaic@localhost` broke mid-word with
    half the row empty beside it until both were told to take the space that was there.
    The lock-wait card is the same lesson in the other direction: it lost its right border to
    `width: 100%` *plus* a 12px margin, which is 24px too wide because margins sit outside the
    width — column-flex stretch subtracts them and a percentage does not.
  - `plan_view.rs` — Query Plan modal (`EXPLAIN`/`EXPLAIN ANALYZE` table + warnings + "Ask AI"),
    via `TabsActions::run_plan` → `Db::explain`.
  - `properties.rs` — the **table properties** modal (`properties_overlay`), opened by setting
    `overlay.properties` — from a Table or View row's context menu, or from the RESULTS title bar's
    Properties icon for a tab with a source table; an effect in the modal calls
    `SchemaActions::table_stats`, whose result lands in `overlay.properties_state`. Both entry
    points go through `open_for_table`, which takes the **connection explicitly** rather than
    reading the active one: a query tab keeps the connection it was opened on, and the fetch keys on
    `db_for(target.conn_id)`. Sizes, row
    estimate, the storage-split bar, table options and the index list — deliberately **not**
    structure, which the tree, the designer and Generate DDL already show three times over. The
    column/key counts and the collation come free from the in-memory `TableInfo`
    (`table_designer::loaded_table`), which is what gives a view something to show on an engine
    that publishes no statistics for one.
    - **Qualifying the figures is the feature.** An estimate prints `~4.21m` via
      `RowCount::label`, `Freshness::note` prints the staleness caveat in words, and an index is
      called unused only where `IndexStats::is_unused` says so — worded with its window attached,
      because the counter resets when the server does. An *uncounted* index says "usage not
      counted"; blank would read as none.
    - **Count rows** (`SchemaActions::count_rows` → `Db::count_rows`) is a button because it is an
      uncapped `COUNT(*)`. Its result is folded into the loaded `TableStats::exact_rows` rather
      than kept beside it, so the headline and the Markdown copy read one figure. Its lifecycle is
      separate from the fetch's (`properties_counting`/`properties_count_err`) for the opposite
      reason: a failed count must not replace statistics that loaded.
    - Both requests outlive a close, so each checks `overlay.properties` still holds the target it
      was asked about before writing.
  - `import_view.rs` — the file-import modal (schema context menu → **Import**), over
    `core::import`. Two steps (Source → Mapping) in one panel driven by the `ImportUi` bundle;
    `SchemaActions::import_probe`/`import_run` do the file + DB work off the UI thread. A probe or
    an import can outlive the modal, so both callbacks check `ImportUi::generation` (bumped on
    every open) before writing — and a probe checks `probe_seq` (bumped per *request*) too, via
    `import::probe_verdict`: several probes of one file are routinely in flight, they report in
    completion order, and only the newest may write. **Discarded whole, `busy` included** — that
    flag staying set is what keeps Next and Import disabled until the newest answer lands, which
    is in turn why `run_import` may send a fresh `read_config` beside the *stored* mapping.
    A schema change goes through `import::target_survives`: closing needs positive evidence the
    table is gone, since `load_schema` empties `db_nodes` before it fetches, and a running load is
    *cancelled* rather than abandoned. The effect that re-probes on a settings change tracks only settings
    that change how the file *parses* — the NULL rules apply at coercion time, so tracking them
    would re-read the file per keystroke and stamp over a hand-edited mapping. While a load runs,
    the footer's Cancel fires `SchemaActions::import_cancel` (the app owns the token, as it does
    for query runs) instead of closing — the transaction rolls back, so a cancelled import writes
    nothing.
  - `table_designer.rs` + `ddl_preview.rs` — **schema editing**, over `core::ddl`. The
    designer is a list-plus-form per section (Table / Columns / Indexes / Foreign keys / Checks) over
    one `DdlUi::draft`; the footer's change count *is* `ddl::diff` of that draft, the same
    call the preview emits from, so the two can't disagree. The list re-renders on every
    draft change but the **form must not** — it seeds local signals from the draft and writes
    back through effects, so a draft-keyed form would tear down the field being typed into
    (it's keyed on `(tab, selected, rev)`, where `rev` is bumped on structural edits because
    removing the selected row leaves `selected` unchanged over a different item).
    It also owns two things the other editors reuse: `list_pane` — the list-plus-action-bar that
    is **one** Tab stop with Up/Down inside it (over `widgets::list_step`) — and
    `focusable_owned_dropdown`, the picker for a value that isn't `Copy`.
    `preview_proposal` is the AI's way in, beside `preview_change`'s: it seeds from `loaded_table`
    like every editor, applies the ops with `propose::apply`, diffs against
    `Target::new(dialect, db_flavour(…))` — the flavour matters, or the same change would preview
    differently here than in the designer — and opens the same modal. It returns `Err` rather than
    raising anything, because the caller is a card in the chat and that is where the user can see
    what the model asked for. Going through `loaded_table` is what makes it refuse mid-refresh,
    which matters more on this path than any other: the model may have read the table minutes ago,
    and a draft seeded from a stale `TableInfo` is how an `ALTER` comes to restate an old column
    definition and silently revert a change that already landed.
    **The form asks the engine for a capability, not "is this PostgreSQL".** It asked the latter in
    three places, which put SQLite on the side that gets a storage engine, a table collation, table
    and column comments and `ON UPDATE` — it has none of them (a collation is per column there, and
    `ON UPDATE` is MySQL's timestamp attribute), so each was a field whose input the emitter then
    dropped on the floor. Column reorder arrows are the one place the old question happens to be the
    right one and stay offered on SQLite: MySQL moves a column with `AFTER`, SQLite gets there by
    rebuilding — the new table is created in the draft's column order, so a move costs nothing
    beyond the rebuild already under way — and PostgreSQL has neither, where an arrow would promise
    an edit no statement can carry out.
    **Every path ends at `ddl_preview`** — designer, Create table, and the context-menu
    shortcuts — so there's one place that shows the SQL, one that names what's destroyed, and
    one "Open in editor" escape hatch. Never run generated DDL without it.
    `DdlPreview::withheld` (from `ChangeSet::unsupported()`) is the other half of that honesty:
    when the dialect can't express part of the plan the modal shows a "This engine can't express
    part of this plan" block listing each one, and Apply refuses — **both on the button and inside
    `apply()`**, the same rule the write guard follows, since a disabled button is not a guard. It
    is a block of its own rather than a line in the risk list because it says the opposite thing —
    the risk block warns what will happen, this one what won't — and it is the call
    `Change::KeepLossyIndex` already established: the SQL is *less* than the change list above it,
    and a preview that didn't say so would be the dishonest half of a destructive operation. Only
    SQLite produces one today, and two things reach it. The column row's Drop is the shortcut: it
    goes through the draft, so it takes the column's dependents with it, and a foreign key or the
    primary key among them is a `Change` that engine can't express — the preview then says so and
    refuses, rather than dropping the column out from under them. The other is a designer plan that
    rebuilds a table carrying an index `IndexInfo::lossy` marked **and cannot replay it** — it was
    edited, it has no `CREATE` text of its own, or the plan moves a column that text may name — so
    the block names it and Apply refuses until the user drops the index or undoes the edit. An
    untouched one is simply replayed and never reaches this block. Entry
    points:
    `table_designer::open_for_table`/`open_for_new`/`preview_draft_edit` (a shortcut whose
    edit has dependents — dropping a column takes its index and FK with it) and
    `ddl_preview::preview_change` (a lone `Change`).
    **`open_for_table` takes a `DesignerFocus`** — `Table`, `Column(&str)` or
    `Key { index, foreign_key }` — which is the row the designer lands on once it opens, so the
    tree's column and key right-clicks (`Edit column`, and `Edit index` / `Edit foreign key` /
    `Edit primary key`) put you on the row you clicked instead of on the table summary with the row
    still to find. It is **named, not positional**: the designer's sequence is the draft's, and the
    draft doesn't exist until the modal opens. That is also why the two cases resolve on opposite
    sides of the open — a `Column` against the introspected `TableInfo` before it moves into the
    `DesignerTarget`, a `Key` against `ui.ddl.draft` *after* `open_designer` returns, through
    `ddl::TableDraft::find_key`, since the seeded draft is the sequence the Indexes and Foreign keys
    lists actually render (and a `PrimaryKeyColumn` answer lands on the Columns tab, the key being a
    tick there rather than a row). Landing itself is the three signal writes any selection change is:
    `ddl.tab`, `ddl.selected`, and the `ddl.rev` bump the form is keyed on. A focus that resolves to
    `None` — a name the draft doesn't hold — is dropped **silently** and the designer opens on the
    `Table` tab, which is what it did before it was asked at all: the request that failed is the
    landing, not the edit.
  - `view_editor.rs` — the **view** modal (tree "Edit" on a view, "Create view" on a database/
    schema node *and* on the editor's right-click when the statement under the caret can be a
    view body — `ddl::can_be_view_body`, which seeds the draft with it), over `core::ddl`'s
    `ViewDraft`. Not a designer tab: a view is a name and a
    `SELECT`, so it's one form on the shared modal chrome, ending at the same `ddl_preview`.
    Same seed-local-signals-then-write-back rule as the designer (the form is built once per
    open; only the footer is keyed on the draft). The options are shown because they're
    *carried* through a replace, and the PG "re-create instead of replacing" toggle is the
    override for the cases `ddl::pg_replaceable` can't read off the statement.
    The form is built **per engine** — check option for MySQL and PostgreSQL, the security/definer
    block for MySQL, the re-create toggle for PostgreSQL, and a SQLite-only "Column names" field
    for `ViewOptions::column_list`. `needs_algorithm` asked `!= Postgres`, which sent a SQLite
    connection off to fetch a `SHOW CREATE VIEW` algorithm; it asks `== MySql`.
    `is_editable_view` is the entry point's gate — a materialized view is drop-only.
  - `window_chrome.rs` — the client-side window decorations: the caption buttons (minimize /
    maximize-restore / close), the drag strip, and the eight resize zones. Draws what
    `core::window_chrome::Chrome` decides, and contains no `cfg!(target_os = …)` of its own.
    `WindowChrome` holds the `WindowId` plus a `maximized` mirror — `is_maximized()` is a query, not
    a signal — which the root's `on_resize` re-`sync`s, so the glyph follows *every* route to
    maximizing (the button, a drag to the screen edge, Win+Up). Three shapes are load-bearing:
    the drag strip is **its own view between the header's clusters**, because Floem dispatches
    `PointerDown` to the deepest view first and `on_click_stop` stops `Click`, never `PointerDown` —
    a drag handler on the header itself would fire on the connection switcher; drag and
    double-click are decided in **one** handler off `PointerDown`'s multi-click `count`, since
    starting an OS move loop on press can eat the second press before `DoubleClick` fires; and
    close calls `close_window`, **not `quit_app`**, because only the former runs
    `WindowHandle::destroy` → `WindowClosed` → `flush_session`, the write that saves open tabs — the
    same reason the auto-updater hands over with `close_window` rather than Velopack's
    `apply_updates_and_restart` (`app/update.rs`).
    The eight zones are spread into the stack that wraps the app root as **loose siblings** —
    never under a full-window parent, which would swallow every press in the app (see *Floem 0.2
    gotchas*, "a full-window sibling ends the pointer walk"); they wrap the root rather than
    joining its tuple because that one is at Floem's 16-arity limit.
  - `trigger_editor.rs` — the **trigger** modal, over `core::ddl`'s
    `TriggerSetDraft`. Reached from the schema context menu's per-table
    **Triggers…** entry — and from a **view's**, on every engine but MySQL, since `INSTEAD OF`
    lives on PostgreSQL and on SQLite, where it is the only way a view is written to at all
    (`overlays::object_entries`, which still excludes a materialized view: PostgreSQL refuses one
    outright); same chrome, same seed-local-signals-then-write-back rule and same
    `ddl_preview` ending as `view_editor`. The trigger modal is the **designer's list-plus-form
    shape** — the table's triggers on the left, the selected one's form on the right, `+`/`−`
    under the list (no ↑/↓: list position is display order, while firing order is MySQL's
    `FOLLOWS` and PostgreSQL's alphabetical) — so one plan can drop one trigger, edit another and
    add a third. It shares the designer's `selected`/`rev` signals, since only one of the two is
    ever open, and splits list-vs-form re-rendering the same way for the same reason.
    **It is deliberately not a designer tab**: what belongs there is what can be a *clause* of
    `ALTER TABLE`, which is why checks are and triggers aren't — a trigger needs its own
    statement, so folding it in would turn MySQL's one coalesced `ALTER TABLE` into an `ALTER`
    plus N statements that commit one at a time, and `DdlError::applied` would stop meaning much.
    It reaches `routine_editor.rs` and used to *contain* it: a PG trigger has no body, only a
    **function** to call, so the trigger form would be a dead end without a way to write one. The
    function modal lived here for that reason and moved out when routines became browsable in
    their own right; what didn't change is the handoff below. Three rules are written down because
    each was a bug waiting: **the form is
    per-engine because the objects are** (MySQL owns a body and one event; PG calls a function,
    takes several events and a `WHEN`; SQLite owns a body, one event, a `WHEN` and `UPDATE OF`
    columns through a "Of columns" field, and offers a view only `INSTEAD OF`), so it *hides* what
    an engine can't express rather than offering it and failing at apply — which is also why
    `blank_trigger`/`trigger_list` take a `SqlDialect` rather than a `pg: bool`, and why the
    MySQL-only `fetch_sources` (`SHOW CREATE TRIGGER`) is gated `== MySql` — whose reply is also why
    **this overlay's `dyn_container` key is deliberately not the memo the routine and view editors
    took** (*Floem 0.2 gotchas*): `form` is built once per `(selected, rev)` and its Body seeds at
    build, so the rebuild that key causes is the only delivery of the escape-corrected body;
    **the function list is re-fetched on its own** (`Db::trigger_functions`
    via `TriggerFnFn`) and arrives a round trip late, so the
    picker keeps whatever the draft already names instead of selecting the first entry and silently
    re-pointing the trigger — the schema fetch carries the same list, so this is what puts a
    function *just created* in the dropdown before the next reload; and **the trigger target is
    never cleared while the routine modal is
    up** — its overlay just renders nothing — so closing that one reveals the half-filled trigger
    form intact, with no "return to trigger" flag to be a second source of truth. `is_editable_trigger`
    is the entry point's gate: a constraint trigger's deferral settings aren't modelled, so it is
    listed and droppable but not editable, the call a materialized view gets.
  - `routine_editor.rs` — the **stored routine** modal: one form for a function or a procedure,
    over `core::ddl`'s `RoutineDraft`, on both engines that have them. Reached from the schema
    tree's Functions/Procedures folders (row **Edit**, folder **Create**), from the database and
    namespace **Create** submenus, from Find-Anywhere, and from the trigger editor's
    **New function** / **Edit** buttons — which is the path it was born on and the reason
    `open_for_new_trigger_function` is a separate entry point from `open_for_new`: a trigger
    function starts from a different draft (`plpgsql`, `RETURNS trigger`, and the `RETURN` that a
    first one most often fails at runtime for want of). Same chrome, same
    seed-local-signals-then-write-back rule and same `ddl_preview` ending as the editors above —
    **with one exception, and it is the Body field.** Its text lives on `DdlUi::routine_body`
    (`ui.ddl.routine_body`), owned
    outside the form and written by `bound_field_on` (`bound_field` over a caller's signal) rather
    than by a view-local one, because a field only the user writes is not what this is: MySQL's
    `SHOW CREATE` reply lands after the form is up and has to correct it. Routing that correction
    through the overlay's own `dyn_container` key meant the modal was torn down and rebuilt to
    deliver it — the seeding *was* the delivery, so the write order inside `done` was load-bearing
    with nothing saying so and nothing testing it — and a reply arriving mid-word took the caret
    with the old scope. `edit_field` now reconciles its doc from the signal in place. The
    write-back contract is unchanged: `prev` is `None` on the first run, so seeding is never
    mistaken for an edit, and an external correction reaches the draft exactly as a keystroke does.
    Three things are load-bearing: **the form is per-engine *and* per-kind because the objects
    are** — volatility, strictness, language and per-routine `SET` clauses on PostgreSQL;
    determinism, data access, definer and comment on MySQL, which has no `LANGUAGE` clause at all;
    and neither volatility nor strictness on a PostgreSQL *procedure*, whose `CREATE` rejects both
    outright — so it hides what an engine can't express rather than offering it and failing at
    apply, which is a rule the kind axis needs as much as the engine one. The Language dropdown
    carries the routine's own language alongside the two it proposes, for the same reason: a list
    that didn't would silently retype a `plpython3u` function the moment the control was touched.
    **MySQL's body is fetched a
    second time and it is not an optimisation** (`Db::routine_source` via `RoutineSrcFn`, applied
    to *both* sides of the diff so a routine doesn't open already-changed, and guarded by
    `DdlUi::session` so a slow reply for a closed modal can't land); and **a new routine's
    namespace is inherited, not chosen** — there is no Schema field, for the reason the view
    editor has none, and the title is where the inheritance is disclosed. It clears every sibling
    overlay's target on open **except the trigger editor's**, which is the handoff described
    above.
  - `object_editor.rs` — the **enum / domain / sequence** modal, over `core::ddl`'s
    `ObjectDraft`. Reached from a tree object's **Edit**, from a database or schema node's
    **Create ▸ Type / Domain / Sequence** (PostgreSQL only — on MySQL those entries don't
    exist, the same "hide what an engine can't express" call `trigger_editor`'s form makes;
    `overlays::create_submenu` is the one builder both nodes' Create submenu comes from), and
    from the folder that holds the objects themselves — `Types`/`Domains`/`Sequences` each offer a
    flat, kind-named **Create sequence** / **Create type** / **Create domain** of their own, which
    calls `open_for_new` directly rather than through `create_submenu`: the folder has already
    said which kind, so the submenu's other two children would be entries belonging to the folders
    either side of it.
    One modal for three objects because the chrome, the footer, the change count and the
    ending at `ddl_preview` are identical and only the middle section differs. **A routine is
    not one of them**, and the split is made at the type level rather than by convention:
    `ObjectDraft::from_item`/`blank` return `None` for a routine, and this module's own
    `open_for_object`/`open_for_new` are what route one to `routine_editor` — so the tree, the
    palette and the menu all keep asking one function to open an object, and a routine handed
    to a type form would have to be an explicit mistake rather than whichever arm compiles.
    Same
    seed-local-signals-then-write-back rule as the other editors, and three more written down:
    the list rows are keyed on `object_rev`, a **structural** counter, so typing into a row
    doesn't tear it down and removing one doesn't leave its neighbour showing the old text;
    an enum's values are **rows, not a newline-separated box**, because a label may contain a
    newline and splitting one would rebuild the type around the split (data loss on apply,
    not a failure); and a sequence's numbers go through `object_errors` beside the draft,
    since the draft holds `i64` and a half-typed `-` has nowhere to live in it — writing
    nothing back on a failed parse would silently swallow what somebody typed.
    `is_editable_object` is the entry point's gate: an identity column's counter is listed
    and alterable but not editable-as-an-object, the call a materialized view gets.
  - `ai_panel.rs` — AI Assistant panel (`ai_panel`/`message_bubble`/`render_segments`/`tool_chip`/
    `assistant_footer`). The two roles are drawn **asymmetrically**, and deliberately: the user's
    question is a shrink-wrapped right-aligned bubble on `bubble_user_bg`, while Claude's turn has
    no surface at all — full-width prose on the panel, marked only by a 2px `accent` rule down its
    *right* edge, with the cost footer separated by space rather than a rule. That is why there is
    no `bubble_claude_bg`: the assistant's text (`bubble_claude_text`) is a foreground with no
    paired background, and `contrast.rs` audits it against `bg_panel`. The rule is a setting
    (`AiUi::gutter`, persisted as `UiState::ai_gutter`, flipped in AI Settings); the padding that
    clears it is read from the same signal, so turning it off gives the reply equal insets instead
    of leaving it off-centre. It is also the one AI setting outside `ai_settings_now()`, so flipping
    it never respawns the session. Two views carry an attachment: `attachment_chip` sits over the
    message box showing what the *next* question will send, with an × that is a real cancel — the
    last point before rows leave the machine — and `sent_attachment` sits above a past question
    showing what it *did* send, expanding to the exact block the model was given rather than a
    re-rendering of the grid, so "what did I send?" is answered by looking. A restored conversation
    has the summary without the rows and says so (`Attachment::retained`). The chip's
    `dyn_container` keys on the summary alone, never the rows: a key clones what it reads on every
    notification. **`sent_attachment`'s expand flag is handed in, not owned by the card** — it lives
    in the `dyn_stack` item's scope beside `pop`, because the bubble's own `dyn_container` is keyed
    on `(fingerprint, is_last)` and the theme generation, and neither a reply arriving (which flips
    `is_last` on the question above it) nor a theme switch is about the card. A view-local
    `RwSignal::new(false)` is a *new* signal after each rebuild, so a card the reader had opened to
    check what was sent snapped shut the moment the answer landed. That scope outlives the rebuilds
    and dies with the message.
  - `overlays.rs` — absolutely-positioned popups: connection/active-db/schema menus, schema context
    menu, generic grid popup, Find-Anywhere, error modal.
  - `schema_tree.rs` — SCHEMA sidebar (`schema_panel` + db/table/column/key row builders + keyboard
    nav). The standalone objects hang off the same levels the tables do, in
    `Types`/`Domains`/`Sequences`/`Functions`/`Procedures` folders after them
    (`object_groups`/`object_group_node`/
    `object_row`, over `schema::ObjectItem`, keyed by `ddl::ObjectKind::ALL` — one list, because
    there were four copies of the kind array across the folder builder, its two filter predicates
    and Find-Anywhere, and a kind added to three of them is a kind the palette silently cannot
    find). An empty folder isn't rendered; the first three exist on PostgreSQL only and the last
    two on every engine with stored routines, so a SQLite tree grows none of them. They are
    scoped by `TableScope` for the reason
    it exists — *flat* means the database has no schema level, not that its objects have no
    namespace. Two filter rules follow from the level above being evaluated first: a database
    and a namespace both survive a search that only one of their **objects** matches, or the
    match would be hidden by the row that contains it. `nav_rows` carries the folders and their
    leaves like everything else — it is the function that must stay bug-for-bug identical to
    the render.
    **What the eye hides, it hides from every surface — and the rule is two predicates in
    `core::schema`, not a filter each surface remembers.** `db_visible` answers "may a *list* show
    this database": the tree's `dyn_stack`, `nav_rows`, the QUERY toolbar's selector menu and the
    trigger that opens it, and **Find-Anywhere**, which had spelled it `!hidden.contains(…)` inline.
    The two agree today — `db_visible` *is* that expression — but the palette is the surface
    `core::db_hidden`'s module doc names first among those a hidden database must disappear from,
    so it is the one that would silently stop following the rule the moment the rule grew a clause
    (an active-database exception, a case-insensitive match, a per-namespace key).
    `db_contributes` answers "may a surface that *describes* the schema use
    it": autocomplete's `SchemaIndex` and the AI's prompts, where a hidden database supplies no
    name, no table and no column. They live in core because three crates ask them — the tree and
    the selector in `schemaic-ui`, the prompt builders in `schemaic-app` — and the rule was
    originally a filter inlined per surface, which is exactly why the selector, autocomplete and
    the assistant each went on showing databases the user had deliberately put away. **The set they
    take is one connection's**, resolved by `db_hidden::names_for` from the persisted
    `(conn_id, database)` rules: the store was a flat `Vec<String>` of bare names, so hiding `world`
    on a MariaDB connection took a live `world` off every other server at once — out of the tree,
    Find-Anywhere, the selector, autocomplete, the assistant's context and `list_schema`'s overview
    — with nothing on the second connection saying why. `db_visible` stays a two-argument predicate
    over a `HashSet<String>` because that is still the question each surface asks; the app derives
    the set as a memo over `(active_conn, the rules)`, the shape the activity interval already used,
    so a connection switch repoints every consumer together. "Is anything
    left to offer" is the same filtered set, not the raw node count, or hiding the last database
    leaves the selector's open flag set with no panel to answer it. Three deliberate exceptions.
    The **eye's own menu** must list a hidden database for it to be unhidden. The **database being
    worked in** survives both predicates (`db_contributes`, and `shown_database` for the toolbar
    label): hiding it doesn't move the tab, so a label that stopped naming it would lie about where
    the next query goes, and a completion or an assistant blind to its schema would be useless in
    it. And **`build_catalog` never filters** — that catalog is what the diagnostics squiggle
    against, and a hidden database's tables have not stopped existing, so filtering there would
    mark `archive.orders` as unknown over a view preference. Hiding governs what is *offered*,
    never what is *true*. The same line runs through the built-in **MCP server**: `list_schema`'s
    *overview* asks `mcp::listed_databases` (so the assistant's tools agree with its prompt about
    what this session can see), while `list_schema {"database": …}`, `describe_table` and
    `run_query` answer in full — a named lookup is a fact, and half-filtering a fence `run_query`
    walks through anyway would be theatre. The MCP subprocess is handed a **blob, not a signal**
    (`hidden` on the endpoint JSON, absent in older blobs → nothing hidden), so hiding a database
    mid-session takes effect on the next one, exactly as the system prompt beside it does. **A folder now carries a menu of its own** (`CtxKind::ObjectGroup`), reversing the
    earlier call that a structural row should offer nothing on either the pointer or `Shift+F10`:
    it is the script for everything in the folder and `Refresh`, then the one thing you come to a
    folder for — `Create {kind}`, flat and lower-cased to match the object row's `Edit sequence`.
    It exists because creating a sequence was reachable only through the database node's
    `Create ▸`, two levels away from the folder named after it. `object_group_node` stages it
    through the same `CtxOpener` its `on_secondary_click_stop` calls and hands that closure to
    `with_nav_scroll`, so the right-click and the keyboard cannot offer different menus for the
    row.
    - The **size column** (`size_badge`) puts each table's on-disk size at the right edge of the
      *panel*, from the same `core::stats` figures the properties modal shows. It answers the
      question that modal cannot — *which* of these is the big one — and is off by default behind
      SCHEMA gear → **Show table sizes** (`SchemaUi::table_sizes`, persisted as
      `UiState::show_table_sizes`). **That row is absent on an engine with no sizes to show**
      (`schema_settings_overlay` asks `stats::supports_table_stats`, the same capability guarding the
      fetch): on SQLite the column stays empty whichever way the setting is left, so a row that
      visibly toggles while nothing changes is worse than no row. Absent rather than disabled, since
      there is nothing here for another connection to enable — and the setting itself is untouched,
      being global and persisted, so a MySQL connection comes back to whatever it was left at. The panel and not the row, because those are not the same edge:
      the badge started as a `flex_grow` spacer pushing the size to the end of its row, and tree
      rows stretch to the *widest* row (the tree is deliberately not `width_full` so it can scroll
      horizontally), so expanding any table — whose column rows are indented and carry a type —
      widened every row and carried the whole size column past the viewport, reachable only by
      scrolling sideways. It is now out of flow and anchored to the panel: `absolute()`,
      `inset_left(0)`, `width(tree_row_min_w())`, `justify_end()`, the same value every row already
      uses as its `min_width`. `inset_left` rather than `inset_right` precisely because the right
      edge is the one that moves. The trade is that a table name long enough to reach the panel
      edge now runs *under* the size instead of pushing it along — tolerable because sizes appear
      on table rows only, so there is no column row beneath to collide with. What runs underneath
      is also the chevron and the name, so `.pointer_events(|| false)` on the badge is load-bearing
      and not decoration: without it the badge, as the row's last child, won Floem's back-to-front
      pointer walk for the panel's full width and clicking a table stopped expanding it — with the
      column *off* too, because the empty state is still a full-width box. Keyboard nav kept
      working throughout, which is the tell that it was dispatch and not the toggle. The mechanism
      is under *Floem 0.2 gotchas*. `ConnNode::stats` holds one database's `SchemaStats` and is
      both the fetch's trigger and its guard: the app's `fetch_db_stats` fetches only nodes at
      `DbStatsState::Idle`, moving each to `Loading` before
      spawning, and settles a failure at `Unavailable` rather than retrying on every expand.
      **Two things ask it, and the slot is what keeps that from being two queries.** The size
      column's effect asks for the databases that are *expanded* with the column on; a capped
      result's toolbar asks for its own through `SchemaActions::db_stats`, which resolves the node by
      name — that route exists because the column is opt-in while `1,000 of ~4.2m` is not, and
      without it the line would only ever appear for users who had already switched the column on.
      `db_stats` refuses a connection that is not the active one: `db_nodes` is the active
      connection's tree, and another server's databases are named in the same words.
      `start_fetch` resets a node to `Idle` **and bumps `stats_gen`**, and it is the pair that makes
      Refresh both the way to get fresh figures and the only thing that retries a failed one. The
      reset alone never did it: the effect reads each slot `get_untracked` (it *writes* those slots,
      so tracking them would re-enter it mid-loop), so on both refresh paths the column simply went
      blank until something unrelated — the toggle, an expand, a connection switch — happened to
      re-run the effect. A table with no size renders nothing — a dash down a tree of views and
      SQLite tables is worse than a blank.
    `completion.rs` — SQL autocomplete: the ranking + popup layer
    (`recompute_completions`/`accept_completion`/`completion_popup` + `SchemaIndex`/`fuzzy_score`)
    over `schemaic_core::intel`'s scope/context engine.
  - `tabs.rs` — query-tab strip, and where a **`.sql`-backed tab** shows itself. The state behind
    that is four signals on `Tab`: `path`, `disk_sql` (the file's text as of the last open / save /
    reload — `None` means *unknown*, which reads as modified, the safe direction), `file_format`
    (the `SqlFormat` a save has to put back, and whether the read was lossy) and `reload_gen`. `Tab::title` falls back name → file name → "Query N", the user-assigned name
    winning on purpose, since renaming a tab is an explicit act a Save As shouldn't silently undo;
    `Tab::modified` is `path.is_some() && query != disk_sql`, and so always false for a tab with no
    file — an ordinary tab is session-persisted and has nothing to be unsaved *against*.
    **The chip says two different things and says them two different ways**, which is the whole
    design: *being* a file is a standing fact, so it gets a glyph — a dim 14px lucide `file` leading
    the title, tinted `tab_close` like the trailing ×/pin so it reads as chrome; *having drifted* from
    that file is transient, so it gets **italic title text** and no glyph at all. Both markers as
    dots was the first attempt and the wrong one on this chip, because a tab can already carry the
    DB-identity dot: the strip ended up with two dots of unrelated meanings a few pixels apart.
    Italic also costs no width, so unlike `TAB_DOT_W` and `TAB_FILE_W` — neither of which is inside
    `TAB_TITLE_AVAIL`'s 40, so a title has to shed whichever of them is showing or a full-width one
    pushes the × past the chip cap — the slant needs nothing shed for it. (Truncation is measured
    upright: an italic face is a hair wider, and being a hair late to ellipsize isn't worth a second
    text measurement.) A file tab's tooltip is its **full path**, which subsumes the truncated-title
    case and answers what the chip cannot — *which* `orders.sql` — with "— unsaved changes" appended
    when it is modified, since nothing else on screen explains the slant.
    The chip's content is a `dyn_container` keyed on
    `(editing, title, pinned, modified, path-as-string)`. Two of those are less obvious than they
    look: the `modified` read tracks `query`, so the closure re-runs on every keystroke while the key
    only *changes* when the flag flips, which is the one thing that has to rebuild the row; and the
    path is keyed as its display string rather than an `is_some()`, because a Save As from one file
    to another on a tab that also carries a user-assigned name moves neither the title nor the icon,
    and the tooltip would have gone on naming the old file.
    The context menu is three groups, separated: the clicked tab (Pin / Rename / Duplicate / Close),
    then its file (Open file / Save / Save as / Reload from disk), then the strip (Reopen last tab
    / Close other tabs / Close all tabs). Sentence case and no ellipses, like every other entry —
    this menu doesn't mark which entries open something. Open leads the file group because Ctrl+O is
    otherwise keyboard-only: there is no menu bar to put it on. Save is offered on a tab with no file
    too, since it falls through to Save as, which is the answer to "save this" there, while Reload is
    *dimmed* rather than hidden, the way "Reopen last tab" is on an empty ring, so the menu keeps
    one shape.
  - `grid.rs` — the whole results grid (`GridState`/`GridCtx`; `results_view`/`loaded_view` are the
    entry points). `editor_pane.rs` — SQL editor pane
    (`query_pane` + Ctrl+K popup, statement highlight, custom scrollbars). `compute_diagnostics`
    bridges the tab's schema/active-db to `intel::diagnostics`; `syntax_view` draws severity-coloured
    squiggles (red errors / amber typo warnings) with hover tooltips.
  - `erd_view.rs` — the **ER-diagram** canvas over `core::erd`. Edges are drawn by a custom paint
    view (`EdgeCanvas`), *not* a Floem `svg` — `svg` doesn't repaint reliably on reactive change
    here and blanked the edges on drag/hover. Zoom is **semantic, not a paint transform**: cards and
    edges keep logical positions and multiply by `z` only at render, so text stays crisp at any
    zoom. The surface is an infinite free pan (not a scroll view) — drag/middle-drag pans, Ctrl+wheel
    zooms about the cursor, plain/Shift+wheel pans — and hit-testing maps cursor → logical space via
    `(p − pan) / z`.
    A table carrying a `db_color` identity colour has its card **header** tinted with it: `header_bg`
    composites the colour over `theme::erd_node_header` at `HEADER_TINT_ALPHA` (0.22) and is called
    *inside* the header's style closure, so the themable half follows a live theme switch while the
    identity hex arrives by value — the same split `db_color_dot` makes. **Each card's colour is
    resolved once, untracked, before the cards are built**, and deliberately not inside that style
    closure: it re-runs for every card on every pan and zoom, so a scan of the rule list there would
    reintroduce the O(cards²) cost the `positions.with` borrow in `node_card` exists to avoid. The
    lookup needs only the node id, because a diagram is scoped to one `(conn_id, database)` by its
    `ErdTarget` and a node id *is* the display name `TableColorRule` is keyed by. The trade is that a
    card doesn't repaint when a colour changes underneath it — a colour is settable only from the
    schema tree's menu, which this modal covers, and reopening the diagram rebuilds every card. A
    `NodeKind::Stub` card returns before both the header and the border and stays untinted, and the
    *header* alpha is pinned by `contrast::tests::an_erd_header_tint_keeps_the_table_name_legible`,
    which governs `HEADER_TINT_ALPHA` alone, since the header is the one surface where an identity
    colour is a fill under text rather than a 6px dot.
    The tint also carries around the **whole 1px border**, so a coloured card reads as one tinted
    object rather than a tinted band in a neutral frame: `card_border` returns `theme::border()` for
    a table with no colour, and otherwise the colour washed over `theme::erd_node_header()` by the
    pure `tinted_border(tint, header, canvas)`. **The border's wash strength is its own, and is
    picked by the canvas's measured luminance** — `border_tint_alpha` returns
    `LIGHT_BORDER_TINT_ALPHA` (0.60) when `contrast::relative_luminance(canvas) > 0.5` and the
    header's own 0.22 otherwise. That is the same reasoning as "ask a capability, never an engine",
    applied to a palette: a future third theme gets sorted by the property that actually decides the
    answer instead of by being recognised by name. Two strengths are needed because the Light
    theme's header surface and canvas are nearly the same grey (`#EEF0F5` on `#EDEFF3`), so at the
    header's 0.22 a tinted border came out *fainter than the plain `theme::border` it replaced* —
    Amber at 1.09:1 against the canvas where the plain border manages 1.25:1. At 0.60 every preset
    clears it, Amber still the floor at 1.29:1. It can't go much lower — Amber is a pale yellow
    whose luminance sits near the canvas's, so even a full-strength rule only reaches ~1.5:1 there —
    and raising it risks nothing, because unlike the header a border carries no text. On the dark
    canvas 0.22 was already enough (1.67–2.16:1 against the plain border's 1.37:1, every preset
    washing lighter than `#2E303A`), so dark keeps the header's strength and the border there lands
    on exactly the header's colour, which is what makes the card read as one object. `tinted_border`
    takes both surfaces as arguments rather than reading `theme` itself so the alpha choice can be
    measured against every built-in theme and not just the loaded one:
    `tests::a_tinted_border_is_never_fainter_than_the_plain_one` compares every `CONN_COLOR_PRESETS`
    entry's tinted border against the plain border it replaces, in every `UiThemeKind`, and
    `tests::the_border_alpha_follows_the_canvas_not_the_theme_name` pins both branches (both
    strengths reachable from the two built-ins; white → light, near-black → the header's). The first
    is a **comparison, not a WCAG floor**, which is why it lives here rather than in `contrast`'s
    pairing table — borders are furniture, and holding one to a text floor would mean nothing.
    **Ctrl+F opens a find bar over the canvas** (`Find` + `find_bar`, over `erd::search`), carrying
    the editor's and grid's chrome in the diagram's top-right corner. It is a **sibling of the
    canvas layer, not a child**: the canvas is `.clip()`ped and pan/zoom-transformed, so a popup
    inside it would scroll away with the diagram. The flex-grow wrapper that measures the viewport
    is therefore a `stack((canvas_inner, find_bar(…)))`, and because that wrapper *is* the modal
    body, `inset_top(10.0)` already means "10px below the toolbar" without knowing how tall the
    toolbar is. There is **no prev/next pair**, unlike the other two find bars: those step a caret
    through an ordered document, while this lights up every match at once and moves the canvas only
    when `erd::sole_node` names a single card — a "next" button would have to invent a sequence over
    a 2-D canvas before it had anything to do. The bar also carries `on_event_stop(PointerDown)`,
    because the canvas pans on a primary drag and the popup sits on top of it, so aiming for the
    text field would otherwise drag the whole diagram out from under the pointer. That `Stop` has a
    cost the handler now pays: it is returned from inside floem's children loop, so no ancestor
    reaches the default block that would have re-focused the modal root, and floem takes focus on
    every `PointerDown` before dispatch — a press on the bar's padding therefore left
    `app_state.focus` at `None` and the diagram answering no keys. So the handler hands the keyboard
    to the innermost focus root when the press landed **to the right of the field**, and still does
    nothing when it landed on the field itself, which is what its own comment was protecting.
    `Find::dismiss` does the same, covering Escape-in-field and the ✕ together: closing the bar
    removes the focused editor, and floem clears focus *silently* when a focused view is removed —
    after which Escape didn't close the diagram and Ctrl+F didn't reopen the bar until the user
    clicked something.
    One `Memo<erd::Matches>` per diagram recomputes per keystroke and each card derives its own
    `Option<NodeMatch>` memo from it, so a card whose match didn't change doesn't re-render because
    a character was typed elsewhere — memos specifically, since `dyn_container` is built on
    `create_updater` and does not diff, it rebuilds whenever a dependency fires.
    **`Matches` is indexed by node** for that per-card question: scanning the hit list made it
    O(cards × matches), and a one-character term matches every card — 0.65 ms of pure comparison at
    500 cards, before floem does anything. The same reasoning keeps the matched **columns** out of the
    row stack's rebuild key: they travel into `column_row` as a per-column `Memo<bool>` read inside
    the row's own style closure, because the highlight is a colour and an outline. Carried in the
    `dyn_container` selector instead, a broad term rebuilt ~10,000 views per keystroke on a 500-card ×
    20-column diagram — a memo per row costs a cached read on a pan frame, where a scan there would
    have reintroduced the O(cards²) trap this module already documents twice. The pan and the flash
    are **guarded writes** for the same reason: every card's style closure reads both, and `set` never
    dedups, so typing another character that resolved to the same card was two more full restyle
    passes over a canvas that had not moved. Highlighting is
    `theme::match_highlight` on the table name and on the matched column names, the same colour the
    schema tree marks a filter hit with, and deliberately **not** the tree's per-character
    `highlight_text`: that bakes a fixed font size into a text layout, and a card's type scales with
    the zoom, so the name would stop growing with the diagram. A find hit outranks a column's key
    tint — the gold/purple says what the column *is*, which is still true and still on the glyph
    beside it.
    **A hit is marked twice, and only one of the two marks survives a tinted header.** `name_paint`
    is that decision: over the plain header the recolour measures 5.11:1 (Dark) / 5.69:1 (Light),
    while over an identity colour washed at `HEADER_TINT_ALPHA` it measures 3.11–4.04:1 on Dark and
    as low as 4.38:1 on Light — under the 4.5 floor the pairing table sets for this exact site, worst
    of all on the card the search has just panned to and ringed. So a tinted header keeps
    `theme::text()`, which *is* gated there, and wears its **bottom border** in the match colour
    instead: a border carries no text-legibility debt, moves nothing, and is the language the matched
    rows already use one weight down. It is a function so
    `an_erd_header_tint_keeps_the_table_name_legible` can ask what the code paints instead of
    restating a foreground the code has since stopped using — which is exactly how 3.11:1 shipped
    past a test written for this surface one commit earlier. The three plain `match_highlight`
    paintings are ordinary `PAIRINGS` rows.
    **A stub card is a card that can be found**, so the per-card match memos are derived *above* the
    stub branch: `erd::search` matches a stub on its name deliberately, `sole_node` returns it and the
    canvas pans to it, and the early return used to skip both the recolour and the flash ring — a
    search that said "1 match" and marked nothing.
    The pan is a `create_effect` on the matches memo: given a `sole_node` it calls `erd::center_pan`,
    which solves the cards' own `pan + logical·z` for `pan` — pure and tested beside `fit_bounds`,
    the whole-diagram case of the same arithmetic, because a sign slip or a `w` where an `h` belongs
    leaves the sole match off screen while every readout still agrees a match was found. It reads
    viewport, positions, sizes
    and zoom **untracked** so panning can't re-trigger it, then flashes the card for `FIND_FLASH`
    (3s). **The flash ring is an `outline(2.0)`, not a fatter border** — a border is part of the
    box, so widening one would nudge the card's content by a pixel as the ring came and went — and
    it is read with `with`, not `get`, so it doesn't clone a `String` per card on every pan and zoom
    frame. `flash_seq` is bumped per flash so an expiring timer only clears the outline **it** set
    (searching twice inside three seconds otherwise lets the first search's timer wipe the second's
    ring early), and the expiry reads it through `try_get_untracked`, because the modal can close
    inside those three seconds and take the signals' scope with it.
    **The modal's whole keyboard policy is one `on_event(EventListener::KeyDown, …)` on the root**,
    where a single `on_key_down(Escape, …)` used to be, so the order Escape is offered around in is
    readable in one place — the grid's `grid_key` for the same reason. It sits on the same `ViewId`
    as `focus_root_with_ring`'s own Tab handler and does not displace it: `add_event_listener`
    appends, and `on_key_down` was only ever `on_event(KeyDown, …)` with a key filter in front.
    Escape dismisses the find popup first and closes the diagram only when there was no popup to
    close: `Find::dismiss` returns whether it *was* open, and that answer is the whole mechanism.
    Ctrl+F opens it. The field's own `FieldCfg::on_escape` dismisses too and covers the case where
    the field has focus, since floem registers the editor's KeyDown listener with `on_event_stop`
    and it consumes the key outright; the root handler covers the rest of the modal, which is most
    of it, the canvas being pointer-driven — pan, zoom or drag anything and focus has left the
    search box. That mirrors the grid's focus-independent Escape rather than trusting the field
    alone. `modal_frame` takes a `find: Option<Find>` and the two message-only bodies pass `None`:
    nothing to search there, so neither binds Ctrl+F.
    **Export is the toolbar's fourth control**, a `menu_button` raising the app's shared popup
    channel — `overlay.popup_menu`, the only surface painted *above* this modal (it is last in the
    workspace stack for exactly that reason). The menu saves any of the six `ErdExportFormat`s and
    copies the five text ones from a `Copy as` submenu; pressing the icon again closes its own menu,
    off the same `PopupAnchor::BelowIcon` equality the grid's dropdowns use. This menu is what
    turned up the hoisted-submenu bug (*Popup menus*): anchored at the right end of a toolbar, its
    submenu flips left on any window narrow enough, and a flipped submenu used to be unclickable.
    The anchor is the **button's** rect, taken from `on_move` + `on_resize` rather than by adding
    the control's padding back onto the glyph's origin: a menu hung off the 16px glyph's bottom edge
    rides up into the button that opened it.
    `export_scene` is the bridge to `core::erd_export`: it reads the same four signals the cards
    render from (`positions`, `sizes`, `collapsed`, plus the resolved tint) and reuses `rects`,
    `visible_map` and `edge_shapes` — **the very functions `EdgeCanvas::paint` calls** — so a
    dragged, collapsed, colour-tagged diagram exports as itself and cannot drift from the canvas.
    The custom paint view was never the obstacle the TODO thought it was: `edge_shapes` already
    returns a flattened polyline and marker segments, and both drop straight into SVG elements.
    Text is ellipsized here rather than in the core, with `measure_text_px_at`/`_bold_at` — the
    same measurer that sized the cards. The "+N more" note is carried into the export but "show
    less" is not: the first says the card is showing part of a table, which stays true wherever the
    picture ends up, while the second is an instruction to a canvas that isn't there.
    A column *type* is measured **once per distinct string**, not once per row (the `type_w` map):
    each `measure_text_px_at` builds a fresh cosmic-text layout, and a type is the most repeated
    string in a schema — a database is mostly `int`, `varchar(255)` and `datetime` — so per row that
    was one layout for every row of every card, keyed by the string it is a few dozen for the whole
    diagram. The column *names* are deliberately not cached: a hit there would be the exception.
    The capture happens **before** the save dialog opens, not in its callback (`ErdDoc`): the dialog
    is modal and these signals belong to the modal behind it, so a callback that came back for them
    could be reading a disposed scope — and it also means the file is a picture of what the user
    was looking at when they chose the format, the rule the grid's row snapshot follows. **What is
    captured is as little as the UI thread is obliged to do.** `ErdDoc` is `Text(String)` for the
    four text formats and `Scene(Box<SvgScene>, Option<f32>)` for a picture: `export_scene` must run
    here because it measures through floem's font system, but `to_svg` is pure, so building the
    document, rasterising a PNG and writing the file are all the worker's (`ErdDoc::into_bytes`,
    called inside the app's `export_erd` `spawn_blocking`). Measured at 500 tables × 20 columns, that
    took the UI-thread stall between the click and the save dialog from 65 ms to 25.5 ms.
    `ErdDoc::into_text` is the exception and says why: the clipboard is synchronous, so *Copy as SVG*
    is the one caller with nowhere to hand the work and pays for the document here — and it answers
    `None` for a PNG, which that channel cannot hold, which is why PNG is not in the copy menu. The
    outcome lands in the diagram's own `notice_bar` at the canvas's bottom edge rather than the
    app's shared error modal, which is painted *under* this one. A confirmation fades after
    `NOTICE_LINGER`; a failure stays until dismissed, since it is the only place the reason is
    written down. The fade is `flash_seq`-guarded and `try_get_untracked`-read for the same two
    reasons the find flash is.
  - `erd_raster.rs` — SVG → PNG for the ER-diagram export, and the only reason `resvg` is a direct
    dependency (it was already in the tree via floem's `svg` view, same version, so it costs no
    extra compile). It lives in this crate **because of the fonts**: the cards' widths were
    measured against the bundled IBM Plex Sans, so `png_from_svg` loads those exact bytes
    (`fonts::SANS_FACES`) into its own `fontdb` instead of scanning the machine's — deterministic,
    no startup sweep, and the text lands inside the boxes it was measured for on a build server as
    much as on a desktop. `clamp_scale` is the guard that matters: a whole-database diagram can be
    tens of thousands of pixels wide on its own, and the pixmap is RGBA in memory, so the requested
    2× is reduced until the result fits both `MAX_PNG_DIM` and `MAX_PNG_PIXELS` — returning **less
    than 1.0** when it has to, because the alternative is a failed allocation at the moment the
    user asked for the export. It touches no signal and no file, so the app runs it on a worker
    thread (`export_erd`) and writes the bytes it returns.
  - `monitor_view.rs` — the **Live Monitor** modal (`monitor_overlay`), opened from the results
    title bar with the tab's `(conn_id, database, table)`. It renders `overlay.monitor_log` — built
    by the app's poll loop through `core::monitor::diff_snapshots` — as a Time·Action·ID·Data table,
    and owns *none* of the polling: closing the modal flips `overlay.monitor_open` false, and that
    is what stops the loop.
    **The table is built once and diffed, not rebuilt per poll**, and the two halves of that are one
    change. The body's `dyn_container` reads a **memo** of `log.is_empty()`: reading the log itself
    subscribed to it, and `dyn_container` does not diff — so every poll that landed a single change
    tore down and rebuilt the header, both scrolls and up to `LOG_CAP` rows, cloning the log twice on
    the way. The list jumped to the top (a fresh scroll starts at zero, and `follow` is false exactly
    when the reader has scrolled away to read history) and the header desynchronised from the body
    horizontally, so reading back through a live log was impossible except by pausing. And the row
    stack is keyed on `MonitorEntry::seq`, a number `monitor::append_changes` assigns, **not** on the
    list index: at the cap the log slides, so `{0..999}` describes a different thousand changes after
    every poll while the key set stays identical — floem reuses a view whose key didn't change, so
    memoising the selector alone would have frozen the rendered list at the first thousand changes
    while the log and its export went on moving. The unconditional rebuild was what hid that. Three icon buttons sit in the sub-header between the status line and
    the interval dropdown — Pause, Clear, Export — and they join the modal's `FocusRing` at
    tabindex 10/11/12 with the dropdown moved to 13, so a monitor is watchable with both hands off
    the mouse. **Pause holds the fetch, not the loop**: `monitor_tick` reads the three signals and
    switches on `core::monitor::tick_action(open, superseded, paused) -> Stop | Reschedule | Fetch`,
    which is where the decision lives so it can be tested — the arms sat inside a floem-scheduled
    function no test could call, and swapping two statements would have turned Pause into Stop with
    nothing failing. A paused tick **reschedules**, because a pause that unwound the loop would need
    `open_monitor` to restart it and that resets the baseline and the log — the opposite of what
    Pause is for. A closed modal is the only thing that ends the loop; a superseded generation stops
    *without* re-arming, or the old target's loop polls beside the new one forever. The cost is that the baseline ages, so the first poll after a
    resume diffs against the pre-pause table and logs the *net* change stamped at the resume; that
    is the log's standing rule (an entry is stamped when a poll observed it, not when it happened),
    just coarser. **Clear empties `overlay.monitor_log` only** — the app's baseline snapshot is
    deliberately untouched, so clearing loses history you have already read and never a change that
    hasn't been reported yet — **and it asks first**, through the shared `overlay.confirm`, when
    `monitor::discard_needs_asking` says the log has something to lose and no copy on disk. The
    button is one glyph from Export, at the same metric, dimmed by the same predicate. **Closing the
    modal no longer empties the log at all.** It used to, "for tidiness / no stale flash", which was
    harmless while the log could only be watched and became a total unprompted loss the moment the
    same commit made it an exportable record — on Escape, the most reflexive key in the app.
    `open_monitor` resets every monitor signal on the way in, so there was never a stale flash to
    avoid. **Export** raises the shared `overlay.popup_menu` over
    `core::monitor::LOG_FORMATS`, anchored `PopupAnchor::BelowIcon` from the ring wrapper's
    `layout_rect()` exactly as `table_designer::suggest_chevron` does (it paints above the modal
    because `popup_menu_overlay` is mounted after every modal in the workspace stack — only
    `submenu_layer`, which draws this menu's own submenu, sits above it); choosing a format runs
    `save_log`, which mirrors `grid::save_export` in snapshotting the log **before** the file dialog
    opens — the dialog is modal and slow and the poll keeps appending behind it — then renders
    through `log_result_set` and hands an `ExportRequest` to the app's `ExportFn` worker with
    `source: None` and a default dialect, both of which only the SQL renderer would read.
    `status_line(export_err, poll_err, paused, partial, capped)` decides what the sub-header says:
    an error takes the line outright, worst-to-mislead first (a failed export beats a poll error,
    because the user believes they have a file and doesn't), and otherwise it is a lead — `Watching`
    or `Paused` — plus **every** caveat that applies, joined. `partial` (only the first `ROW_CAP`
    rows are watched) and `capped` co-occur and neither replaces the other. `capped` is
    `overlay.monitor_dropped > 0`, accumulated from `monitor::trim_log`'s return, **not** the log's
    length: at exactly `LOG_CAP` nothing has been dropped yet, and the caveat used to be printed
    there anyway. An export failure that lands after the modal has closed goes to the shared error
    modal rather than to `monitor_export_err`, which nothing renders once it is shut, and the
    message is passed through as the pipeline wrote it — a second `Export failed —` in front of it
    read "Export failed — Export failed: Access is denied". `Tone` resolves to a `fn() -> Color`, per the themable-colour invariant.
    It is pure and tested inline, which is what keeps the copy honest: the two caveats co-occurring
    is precisely the case a per-state `match` got wrong.
  - `theme.rs`/`themes.rs`/`icons.rs`/`fonts.rs`/`sql_highlight.rs`.
  - `contrast.rs` — the **legibility gate** over `themes.rs`: WCAG relative-luminance maths plus
    `UI_PAIRINGS`/`EDITOR_PAIRINGS`, one row per (foreground, background) combination the UI really
    paints, each with the floor its role earns. The unit is the **pairing**, not the literal — a
    grep for hardcoded hex can't see a paired accessor whose two halves are chosen in two different
    views, which is how three sites shipped at 1.02:1 in *both* themes. Chrome that is below AA
    today is listed in `UI_SHORTFALL` with the ratio it manages: an unlisted pairing must meet its
    floor (so a new colour, surface or theme is held to AA), a listed one may never get worse, and
    a listed one that now passes must be deleted — the baseline can only shrink. **A baseline entry
    is keyed on `(theme, fg, bg, role)`, the role included**: keyed on the colours alone, *reusing*
    a listed colour in a harder role was invisible to the gate, which is how every form caption in
    the app came to be painted at 2.55:1 under a row baselined for icons. Adding a theme
    needs no work here; painting a role on a new surface means adding its row.
    One background the table **cannot** express is composed at run time: an ER-diagram card header
    carries the table's identity colour washed over `erd_node_header`, and the `pair!(text on
    erd_node_header, …)` row covers only the untinted case. So
    `an_erd_header_tint_keeps_the_table_name_legible` asserts that pairing separately — `text` on
    every `CONN_COLOR_PRESETS` entry at `erd_view::HEADER_TINT_ALPHA` over that surface, every
    built-in UI theme, against the `Body` floor of 4.5:1, worst case 5.0:1 (Amber on Dark).
    `env_badge_text` is excused from the same question because no theme can promise a ratio on an
    arbitrary connection colour; here the colour set is closed and the wash strength is ours, so the
    promise can be kept and is measured. A failure means the alpha is too high, not that a preset is
    wrong. That test covers the **header only** — the same card's tinted border is deliberately not
    asserted here, because a border carries no text and a legibility floor on it would mean nothing;
    `erd_view::tests::a_tinted_border_is_never_fainter_than_the_plain_one` holds it to the plain
    `border` it replaces instead.
  - `lib.rs` (~5.6k lines; `grid.rs` at ~6.3k is the crate's largest) — the `Ui` struct + bundles, shared model/state
    types, `workspace`/`body`/`center`/`header`/`footer`, resize handles, `edit_field`/`FieldCfg`,
    terminal panel. The shared types living in the crate root is what stalls further splitting: the
    root depends on the leaves (`mod`) and the leaves depend on the root (types), so a view builder
    can't move out until the types do.
    **A first launch saves nothing and selects nothing.** `app_view` used to seed a "Local
    MariaDB" connection (127.0.0.1:3306, this repo's development credentials) whenever the loaded
    list came back empty, and write it to disk on the spot — so a fresh install opened onto a
    connection its user never made and probably couldn't reach, and deleting `connections.json`
    only made the next launch write it back. It is gone; the empty list stands, and the header's
    button is what that state offers. `Connection::startup_active_id(saved, &connections)` picks the
    active id — the saved one while it still names a connection, else the first, else
    `next_id(&[])`, **the id the first connection saved this session will take**, because
    `save_conn` loads the schema for a connection only when it is the active one and a new user's
    first connection should connect on save rather than wait to be selected. That last arm was
    unreachable while the seed existed and read `unwrap_or(1)`; the number is the same and the
    coupling to `next_id` is the part under test.
    The header's **connection trigger** is one slot with two occupants, chosen by
    `connections.is_empty()`: the switcher normally, and on a first run with nothing saved a
    `New connection` button in the accent that drafts a connection and opens Manage Connections in
    one press. With nothing saved there is nothing to switch *between* — the switcher reads "No
    connection" and its menu puts the only action a first run has three clicks down (open the menu,
    Manage Connections, New connection), which is a funnel with no traffic at the top. The test is
    "none saved", not "none active": a user with connections and no active one still wants the
    switcher, since choosing is what it is for. Both wear `switcher_chrome`, shared rather than
    spelled twice **because they swap in and out of the same slot** — a margin or radius that
    differed would show as the header twitching the moment the first connection is saved. The
    caller sets the border colour (the active connection's identity colour, or the accent) and may
    re-state the horizontal padding: the button does (9px/10px against 11px/7px), because its
    content is the switcher's mirrored — glyph leading, label trailing — so the plus carries its own
    sidebearing into the left inset while a label ending flush needs more room after it. The pill's
    outer edges still land where the switcher's do, which is the part that keeps the header still.
    The switcher is the *only* thing that sets `conn_menu_open`, so the button leaves no menu
    behind it that nothing can raise.
    The header's **update chip** (`update_state` + `apply_update` on `Ui`, alongside `resources`) is
    built in `header()` and goes in first in the right-hand cluster —
    `h_stack((update_chip, search, help, settings, chrome.controls()))` — because a muted text
    segment in the status bar was not prominent enough for an offer meant to be acted on. It is
    shaped like the connection switcher at the other end of the header: `border(1.0)`,
    `border_radius(5.0)` and an opaque `theme::bg_chrome()` fill, the fill for the reason the
    switcher documents — an outline over a transparent interior anti-aliases on both edges and looks
    blurry. Then `padding_horiz(9.0)`/`padding_vert(3.0)`, `margin_right(26.0)` — 10px more than the
    16px the glyphs keep between themselves, so the chip reads as its own thing rather than a fourth
    member of the search/help/settings run — and `gap(6.0)` between a **13px** `icons::REFRESH_CW`
    and an **upper-cased label at 11px**. The caption is upper-cased because that is what squares the
    chip up: in mixed case the lone capital `R` of "Restart to update" stood against a run of
    x-height letters, so the glyph block was taller on its left than its right and no *symmetric*
    padding could centre an asymmetric shape — the text read as sitting high whatever the numbers
    were. All caps is one uniform band, which centres against equal padding. The glyph is sized
    against that band rather than against its neighbours: four strokes in a circle read heavier than
    the switcher's single-stroke chevron, so matching the chevron's 16px would leave it shouting over
    an 11px caption. The design also called for 0.06em tracking, which **Floem 0.2 cannot express** —
    neither its `Style` nor the cosmic-text `Attrs` beneath it has a letter-spacing property, and
    padding the string to fake it would wreck the metrics the upper-casing exists to fix. It is
    tinted like the glyphs beside it — `theme::text_muted()` resting, `theme::text()` on
    hover — and **the colour is set on the container**, so the label and the `currentColor` SVG both
    inherit one value; `border_color` brightens with them, so the chip reads as one object rather
    than a box with a lit label inside it.
    It renders a zero-footprint `empty()` whenever `UpdateState::label()` is `None`, which is
    most of most sessions, and is clickable only while `is_actionable()` holds — "Updating… 40%" is a
    progress readout, and a click on it mid-download would have nothing to apply.
- `schemaic-app` — `main.rs` wires signals + callbacks and builds the `Ui`; also the built-in MCP
  server (`--mcp-serve`) the AI panel talks to. A query tab's identity is `(conn_id, database)`;
  the app resolves `conn_id` → `Db` at run time (`db_for`), so a tab keeps its connection after a
  switch.
  **Getting past the row cap is a per-tab override, not a fetch mode.** The cap is read once per
  run, so `Tab::row_cap_override` — set by the results toolbar's "read N rows", cleared by the next
  manual run — is all it takes; there is no second path through the DB layer. The label names a
  **number** on purpose: the cap is a client-side cutoff of the result stream (`db::collect_rows`),
  not a `LIMIT`/`OFFSET`, so there is no cursor to advance, "load more" would be a lie, and the
  action re-runs the whole statement at a bigger ceiling — on an unordered query the second read
  can legitimately disagree with the first. `stats::next_row_cap` picks that number: five times the
  rows actually **read** (not the configured cap, which differs whenever a filter or a small table
  stopped the read short), floored at a thousand so a three-row result still offers something worth
  pressing, and rounded up to two significant figures so the offer reads as a figure rather than as
  arithmetic. The re-run goes through `GridState::current_statement`, **not** `apply_grid_query`:
  the latter reports a base it cannot rewrite as a *filter* failure ("not a simple single-table
  SELECT"), and a join is perfectly re-runnable at a bigger cap — telling a user with no filter
  that their filter is at fault is worse than the cap they were trying to get past
  (`an_ineligible_base_is_still_ineligible_with_nothing_to_splice` pins the premise). Clearing the
  override on a fresh manual run is the other half: a raised cap belongs to the result it was
  raised for, and carrying it forward would be the global setting the user didn't change.
  **The statement timeout is a clock wired to the Cancel button, not a second way to stop a
  query.** `RunTimeout::arm` spawns one sleeper racing a `done` token and, when the sleep wins,
  cancels *the run's own* `CancellationToken` — the same one Cancel fires, which each backend
  already honours its own way (MySQL `KILL QUERY` on a second connection, PostgreSQL
  `cancel_query`, SQLite the interrupt handle). Nothing new can go wrong at the database. Three
  things about it are load-bearing. **`0` is off**, read once in
  `core::persist::statement_timeout`, and off is the default — every release before this ran
  statements unbounded, so a default timeout would start killing the long imports and reports
  people already rely on. **Disarming is `Drop`**, because dropping a `CancellationToken` does not
  cancel it: without that, every finished statement would leave an hour-long `sleep` behind, one
  per run, for the life of the process (`a_dropped_watchdog_never_fires` pins it, and fails
  against a tree with the `Drop` body removed). **One watchdog per statement, not per run** — the
  setting says *statement*, so a ten-statement script gets the full allowance each, which in the
  batch path means re-arming inside `run_batch`'s per-statement callback; a statement that does
  expire still stops the batch, because the token it fires is the batch's. A timeout and the
  Cancel button arrive as the identical `DbError::Cancelled`, so the watchdog's `fired` flag is the
  only thing that can tell the user which stopped their query — hence `timeout_message`, which
  quotes the timeout in the very words the settings dropdown uses (`persist::statement_timeout_label`
  is shared for exactly that reason). `EXPLAIN` is bounded too: `EXPLAIN ANALYZE` *executes* the
  statement, so it is as runaway-capable as the query itself, and there the timeout must produce a
  `PlanState::Failed` rather than the plain-cancel `return`, which would leave the modal spinning
  on `Running` for ever.
  **The Server Activity poll is generation-guarded, and only runs while the panel is open *and the
  window has focus*.** One effect watches `(right_panel, active_conn, activity_interval,
  window_focused)`; it bumps `activity_gen`, clears the snapshot when the *connection* changed, then
  arms `activity_poll` — refreshing immediately only when the connection changed or the panel just
  became visible. Focus is in the rule because every tick is a full connect + authenticate against
  the server being watched (one connection per operation, §7), so a panel left open behind another
  window was opening and tearing down a connection every couple of seconds, indefinitely, for
  nobody — the same reason the health poll pauses unfocused. Refreshing on an *interval* change was
  the mirror-image waste: moving a struggling server from 2s to 30s, done precisely to lean on it
  less, fired an immediate extra fetch, and since the generation had just been bumped the in-flight
  guard couldn't suppress it — two `PROCESSLIST` queries at once, on the server that prompted the
  change. Each timer carries
  the generation it was armed under and stops the moment it differs, which is what makes closing the
  panel, switching connections or changing the interval end the old loop instead of starting a second
  one beside it — the same guard the Live Monitor's tick uses, and the same `try_get_untracked`
  discipline, since a pending timer can outlive its signals at shutdown. An in-flight fetch carries
  the generation too and is **dropped** on arrival if it no longer matches: a reply describing the
  previous server, landing under the new one's heading, is a list of session ids the user might kill.
  Clearing the snapshot on a connection change is the same rule stated once more.
  **A switch is not the only way the server under the panel changes, and `reset_activity` is the
  other way in.** The effect keys on `active_conn`, which is an *id*, and editing a connection in
  place does not move it — so repointing the active connection from host X to host Y with the panel
  open left X's sessions on screen, live-looking, under a connection now pointing at Y, and a kill
  from that list would have sent X's thread ids to Y. `save_conn` calls `reset_activity` for **any**
  save of the active connection, without asking whether the target actually moved: comparing would
  mean carrying the old host/port/socket alongside the snapshot — a second copy of connection
  identity kept in step by hand — to save one `PROCESSLIST` query when someone renames a connection.
  **A snapshot is where that trade is right, and it is the only thing in `save_conn` for which it
  is** — see *What an edit invalidates* below, where the same question is asked of a transaction and
  answered the other way. **`delete_conn_now` calls it too**, and for
  the same reason rather than a related one: deleting the *last* connection sets `active_conn` to
  `startup_active_id(None, &[])`, which is `next_id(&[])`, which is `1` — the id the first connection
  ever created holds — so deleting that one leaves the signal on the value it already had and the
  effect sees no change. The panel went on listing a deleted connection's sessions, offering to kill
  them. Two call sites, one for each way an id can fail to move.
  Inside it the generation bump comes first
  and is the load-bearing half, since a fetch already in flight against the old host would otherwise
  land and refill the panel with the rows being thrown away. **The refetch, unlike the clear, is
  conditional on `db_for` succeeding**: clearing is always right, but refetching *now* is right only
  when the connection can be reached this instant, and after an edit it routinely cannot — `save_conn`
  has just dropped the tunnel and `load_schema` re-opens it asynchronously, so `db_for` answers "SSH
  tunnel is not established yet" and refreshing anyway painted that across the panel as `Failed`, with
  no tick to retire it when the interval is off. `Failed` is the right answer for a refresh the *user*
  asked for and the wrong one for a reset nobody asked for, where "no snapshot yet" is the truth; the
  delete path reaches the same guard through `db_for`'s "connection no longer exists". For the same
  ordering reason `save_conn` calls `load_schema` **before** the reset, so the re-open has at least
  been asked for. Both the effect and `reset_activity` take the "is the panel polling" gate from one
  `activity_polling` closure — its reads are tracked, which is what the effect needs and what makes
  them inert at the other caller — because that gate has grown a conjunct before and a second copy is
  how the next one reaches one asker and not the other. A refresh over a
  live snapshot leaves it on screen rather than passing back through `Loading`, or a two-second
  interval would be a panel that flashes instead of one that updates. The interval is the only part
  persisted, and it is **per connection**: `UiState::activity_intervals` holds the rules and
  `activity_interval` is a *memo* over `(active_conn, the store)`, which is what makes the clock's
  tint, the menu's marked row and the timer's re-arm all repoint together on a switch. The panel
  writes through `ActivityActions::set_interval` rather than to a signal, so there is no effect
  reading out of the store and another writing back into it.
  **What an edit invalidates, and how expensive the wrong answer is.** `save_conn` asks
  `targets_same_server` — the predicate the schema tree already asks, not a second reading of the
  same fields — and everything that belongs to the *old* server is torn down only when the answer is
  no: the cached SSH tunnel, every pinned Manual `Session` on the connection, and the live AI
  session. It used to ask nothing: the tunnel was dropped on **every** save, which is true of an edit
  that moved something and quietly destructive of one that didn't, because tearing the listener down
  takes the forwarded connections with it — so changing a connection's *colour* killed the socket
  under a pinned Manual transaction and rolled back uncommitted work while the tab went on offering
  Commit and Rollback. **Cheap-and-unconditional is right for a snapshot and wrong for a
  transaction**, which is the whole distinction between this and `reset_activity` above. The orphaned
  tabs get `delete_conn_now`'s treatment rather than `repair_killed_session`'s — drop to Auto-commit,
  `TxState::closed()`, no prompt, since the server rolls back on disconnect — because a reopen would
  put the tab back on a server that has just gone: `open_session` would fail on the spot, flip the
  tab to Auto anyway and raise an error modal on top of it. The AI session is on the same list
  because `needs_respawn` cannot see a repoint: it compares the conn id, which does not move, and the
  `AiSettings`, none of which name a host, so the assistant's MCP subprocess went on reading the
  previous server — or, tunnelled, a local port that no longer answers. Dropping it costs nothing,
  since `ai_send` replays the conversation into the next session's prompt. `delete_conn_now` drops
  the live AI session too, and not merely for symmetry: it already cleared the *saved* transcript,
  but the running `claude` child survived, and while deleting the active connection usually moves
  `active_conn` and so respawns by the side door, `next_id` is `max + 1` — deleting the
  highest-numbered connection frees its id for the next one created, which becomes active under the
  same id with settings unchanged, and nothing asks for a respawn.
  **A killed session may be one of ours, and the tab has to be told.** A Manual tab pins a
  `Session`, that connection is an ordinary row in the panel, and the idle-in-transaction holder
  blocking another tab is very often exactly it. Terminating it left the tab holding a dead socket:
  the footer went on offering Commit and Rollback, the next statement failed with a connection
  error, and the only way out was closing the tab. `repair_killed_session` matches the killed
  `(conn_id, server id)` against `Session::server_id` across the session map and, on a hit, clears
  the tab's `TxState` and reopens a fresh pinned connection through the existing `open_session` —
  the tab stays in Manual with no transaction open, which is the truth after the kill. **The
  `conn_id` half of that key is not a formality**: a server id is only unique on its own server, and
  MySQL thread ids and PostgreSQL backend pids are small integers each server hands out from its own
  counter, so two Manual tabs on two connections routinely hold the same one. Matching on the id
  alone reached into whichever tab the map yielded first and, when that was the wrong one, closed a
  transaction still open on a server nobody had touched and re-pinned its connection underneath it —
  losing the uncommitted work of a tab the user never acted on, while the tab that actually lost its
  socket stayed broken. **The kill asks `may_launch_destructive` before it raises the confirm** —
  the shared guard every other destructive modal action asks, which this path asked nothing at all —
  and a read-only connection gets the refusal in words rather than a modal it cannot complete.
  The kill itself resolves its target `Db` **when the confirm is raised**, not
  when the button is clicked, for the same reason a session id means nothing without its server: a
  modal is open across an unbounded stretch of time, and reading `active_conn` inside `resolve` sent
  `KILL CONNECTION 1148` to whatever connection was active by then, under a title naming the first
  one. A **cancel** is not a kill and repairs nothing: it stops the statement and leaves the session
  and its transaction alive.
  **Opening and saving a tab's `.sql` file** is `TabsActions::open_sql_file`/`save_sql_file`/
  `save_sql_file_as`/`reload_sql_file` (Ctrl+O / Ctrl+S / Ctrl+Shift+S, bound in `NavKeys::handle`
  so they work at the workspace root and inside the editor alike, and offered as Open File / Save
  File / Save File As in the palette). They are split the way the results export is: the dialog
  (`floem::file_action::open_file` / `floem::action::save_as`, filtered on
  `sqlfile::SQL_EXTENSIONS`) and the tab bookkeeping run on the UI thread, while the read/write goes
  to `handle.spawn_blocking` and comes back through `create_ext_action` — the IO is synchronous and
  a large script would otherwise freeze the window. Every decision about bytes and names is
  `core::sqlfile`. A tab closed mid-flight is detected with `try_get_untracked` and the callback
  degrades to a no-op: the bytes are on disk either way, there is just no tab left to mark saved,
  and reading a disposed signal would panic. A save snapshots the text *before* the write, so typing
  during it correctly leaves the tab modified afterwards. Failures land in the shared error modal
  (`error_modal_text` + `error_modal_open`), because a failed Open or Save has no grid and no error
  bar of its own to land in and silence is the one thing it must not be. Open activates an
  already-open tab on the same connection rather than opening one file twice — two tabs saving over
  each other is a lost edit — and otherwise places the new tab through `place_tab`, whose blank-slate
  predicate gained `path.is_none()`: a tab bound to a file is not a blank slate even when the file
  is empty, since reusing it would drop the binding and the next Ctrl+S would go somewhere else.
  Save falls through to Save As when the tab has no path, and Reload asks through the shared
  `Confirm` channel when the tab is modified, since nothing else in the app can put those edits
  back. None of the four is connection-gated, deliberately and like the export: a file is between
  the editor and the disk.
  **Closing a modified file tab asks too, and `guard_close` is where both close questions live.**
  It has `GuardTxFn`'s signature (aliased `GuardCloseFn`) and wraps `guard_tx`, so it drops into
  every path that already took one — `close_tab` (×, middle-click, Ctrl+W) and `close_tabs_seq`,
  which means Close all / Close other tabs ask per dirty file tab as their chain reaches it. The
  blanket "close all tabs?" confirm is about closing tabs, not about discarding file edits, and the
  chain keeps one question on screen at a time.
  **The order of the two questions is load-bearing, and so is asking neither of a tab that can't
  close.** Answering the transaction prompt is not an answer but an *action* — it commits or rolls
  back — so anything that can still call the close off has to be settled first. `guard_close`
  therefore checks `tabsel::can_close` before it opens anything (a pinned tab, or an id already
  gone), then asks about the file, then hands over to `guard_tx`. Both orderings were wrong before:
  Ctrl+W on a pinned tab holding a transaction prompted, took the commit and then declined to close,
  because the pinned test lived only at the far end in `close_tab_now` — which still refuses, as the
  backstop every close path passes through, but by then the damage is done. The file question is
  raised only on a file-backed tab, since `Tab::modified` is false for an ordinary one.
  `close_tab_now`'s keep-≥1 branch clears `path`/`disk_sql`/
  `file_format` along with the text: the blank slate it leaves behind must not still point at a
  file, or the next Ctrl+S would overwrite that file with an empty document.
  A file tab survives both kinds of restore. `persist::SavedTab` carries `path`, `file_crlf`,
  `file_bom`, `file_lossy` and
  `file_dirty`, each `#[serde(default, skip_serializing_if = …)]` so a session file written before
  the feature still restores its tabs — flat bools rather than a nested struct for exactly that
  reason; `ClosedTab` carries `path`/`disk_sql`/`file_format` so
  Ctrl+Shift+T brings back a *file* tab rather than an untitled copy of its text, and its "worth
  restoring" guard counts a path as worth restoring on its own — the binding is the thing being
  lost, even from an empty file. The file's *contents* are deliberately not persisted: `file_dirty`
  is one bit that lets the restore decide whether the `query` it already has **is** the on-disk copy
  (`disk_sql = Some(query)`) or unknown (`None`, which reads as modified until the next save or
  reload settles it).
  **Quitting the window is the one close path `guard_close` never sees, so it is answered by a
  flush rather than a question.** Floem 0.2 handles `WindowEvent::CloseRequested` by closing
  unconditionally — there is no veto to hang a prompt off — but the close *is* observable
  (`WindowHandle::destroy` fires `WindowClosed` before disposing the scope), so the workspace root
  carries a `WindowClosed` listener that writes the session synchronously. Without it a quit inside
  the 600 ms save debounce left `tabs.json` holding the **previous** save, whose `file_dirty` was
  `false` because the tab was clean then: the tab came back with the pre-edit text, no italic and no
  dot, reporting itself as matching disk. Confidently wrong is worse than stale. The snapshot is
  built by one closure (`session_snapshot`) shared with the debounced effect, because the builder
  that runs at quit is the one nobody watches.
  **And "restore tabs off" is not a licence to drop unrecoverable text.** With the setting off, the
  flush writes — and the restore reads — only `SavedTabsFile::unsaved_files_only`: a file-backed tab
  with unsaved edits, whose text is neither on disk nor retypeable. Every other tab is a query the
  user can retype or a table they can reopen, so nothing else is stored against their preference; the
  same subset is read back so a full session left over from when the setting was *on* isn't silently
  restored either.
  **What the assistant knows about the result on screen, and how rows reach it.** `TurnContext`
  carries a `result` — `core::prompt::result_shape` over `Tab::shown_result`, which is
  `shown_panel` again rather than a second fallback rule (a stale `active_result` shows panel 0 in
  the pane, and the AI has to describe the same statement) — so columns, types, counts, the cap and
  the elapsed time ride on every turn, and *no cell value ever does*. A failed run's error text rides
  only where the connection's `AiData` allows it, since the server quotes stored values into its
  messages; `ai_context` passes the level in beside the dialect. It is
  diffed like the rest of the context, and a cleared panel is reported as cleared rather than
  omitted: a stale shape would have the model explaining an error the user has already fixed. The
  same snapshot sends the editor **selection** in place of the whole buffer when there is one
  (`core::text_ops::selected_text` over the range `Tab::selection` mirrors out of the mounted
  editor), labelled as a selection by `editor_section_label` — one function, because the system
  prompt and every later delta have to agree about whether the model is looking at a fragment or
  the script. Rows themselves travel only as an `Attachment`: the grid's *Attach N rows to chat*
  stages one in `AiUi::attachment`, the panel shows it as a chip with an ×, and `ai_send` takes it
  (clearing the signal, so a second question can't silently re-send it), puts it on the
  `ChatMessage` for the transcript and prepends `prompt_block()` to the turn. The take is gated on
  the level of the connection the turn is **going to**, and the chip is cleared by a connection
  switch and by New Chat: rows are staged against the connection they came from, and the user can
  switch before sending. `ai_regenerate` carries the attachment back into the signal, since the
  rows travel with the turn rather than in `ChatMessage::text` and a regenerated question without
  them asks about data the model cannot see. The whole path is
  governed by the connection's `AiData`: `ai_context` reads the level from the same lookup that
  gives it the dialect — rather than taking it as a parameter that could disagree with the tools
  the session was actually given — `start_ai_session` turns it into the tools list and the MCP
  blob's `samples` flag, and `ai_send` respawns the session when it changes, since both were fixed
  at spawn and a data-access setting that doesn't take effect is the worst kind of lie.
  **Which settings respawn is a rule, and it is `ai::needs_respawn`** — was an inline `need_new`
  closure in `main.rs`, where no test could reach it. The rule is **a setting the spawn froze**,
  which is very nearly all of them. The gravest decide what may leave this machine — `data`,
  `hidden` and `schema_scope`, all three riding in the tools list and the MCP blob, written once at
  spawn. `hidden` is the one that shipped without this: hiding a database mid-session left
  `list_schema` enumerating it and its every table to the vendor while the *prompt* half of the same
  feature updated per turn, so the user watched the assistant stop volunteering the database with no
  way to know the tool it can call still saw it. **`model`, `effort` and `instructions` are frozen
  just as hard** and the rule used to say they could wait: the first two are argv on the `claude`
  child and the third is written into the system prompt, which `ai_context` composes once and every
  later turn only sends deltas against. Nothing was visibly broken, because all three are settable
  only in the AI settings modal and `ai_apply` compared the whole `AiSettings` with `!=` on close —
  a second, blunter rule quietly carrying the case the tested one declined, holding only as long as
  no control outside that modal writes them. A premise, not a design. A different connection is
  always a new session, since the level, the hidden set and the `Db` handle all belong to it.
  **`cli_path` is the one exception, and it is why the rule takes a `cli_usable` argument.** Every
  other setting is a value the app can act on the moment it changes; this one names a *binary*, and
  adopting a name that resolves to nothing trades a working conversation for one that cannot start.
  So the path counts only when it is spawnable — `claude_cli::claude_reachable`: an override that
  resolves, or an empty value whose auto-detect succeeds. Two corollaries, both easy to get wrong in
  the other direction: **manual → empty respawns**, because empty is *auto-detect* rather than
  "unset" and resolves to a binary the session was not started from; and a broken path is **not** a
  licence to ignore the rest, the gate sitting on the `cli_path` comparison alone. The filesystem
  question belongs to the caller because the function is pure — the reason it lives in `ai.rs` at
  all. Both call sites ask it: `ai_send` before a turn, and `ai_apply` when the settings modal
  closes. `ai_apply` used to compare the whole `AiSettings` with `!=` instead, which is a second
  rule for one question and had already drifted — `!=` counts `cli_path` unconditionally, so typing
  a path that resolves to nothing (the state the field's own red hint is for) threw the live
  conversation away.
  **A respawn
  that can't happen refuses the turn** rather than falling through to the old session: with no
  `Db` (a tunnel still coming up) the previous session is dropped and the panel says the database
  is unreachable, because answering through a session built for the previous level is this control
  failing open. The grid
  asks `ai_data_of` (the *result's* connection, not the active one) and checks it at each action as
  well as when building the menu.
  The MCP subprocess gets its DB endpoint as JSON in `$SCHEMAIC_MCP_ENDPOINT` via a
  per-session temp `--mcp-config` file (removed on drop) — never argv, so credentials don't leak
  to other same-user processes. Pure clusters split out: `claude_cli.rs` (`claude` binary
  discovery — PATH/PATHEXT/override) and `ai.rs` (`AiSession`/`start_ai_session` streaming,
  MCP-config plumbing, `ai_context`/`inline_system_prompt`). Reactive wiring (`app_view` closures)
  stays in `main.rs`.
  **Every prompt's database list comes out of one funnel**, `snapshot_databases`: it reads the
  schema-tree signals once into `(database, Some(schema))` plain data, and a database the SCHEMA
  eye has hidden is not in it — not its name, not its tables, not its columns — bar the database
  being worked *in*, the `db_contributes` exception autocomplete makes for the same reason. Both
  the chat panel's context (`turn_context`) and Ctrl+K's generator (`inline_system_prompt`)
  snapshot through it, so neither can be filtered while the other isn't. The filtering half is
  `visible_snapshot`, split out over plain data because that guarantee was enforced by reading
  alone: the funnel takes two signals, so no test could call it, and the two renderers it feeds
  take an already-filtered slice, so theirs never saw a hidden set at all. `db_contributes` was
  tested in core; *that this funnel is what calls it* was not.
  **Ctrl+K's prompt fences and labels the editor buffer and the selection** the way the chat panel
  does — `prompt::fenced_as("sql", …)` plus `UNTRUSTED_NOTE` — because they are the same
  provenance: *Generate DDL* pastes introspected `CREATE TABLE` into a tab, so a column `COMMENT`
  from a server the user does not control lands there. The chat panel's block was fenced and
  labelled while this one, three functions away, was spliced raw immediately before the instruction
  block, and Ctrl+K's output goes into the editor one Ctrl+Enter from running
  (`the_inline_editor_block_cannot_be_closed_from_inside_it`, which also counts the fences so a
  payload's own three-backtick line reaches no margin). And at `SchemaScope::None` the
  withheld-schema note tells the model to use **only what the editor already names** rather than to
  ask the user: Ctrl+K is one `claude -p` with no stdin and no session, under a preamble whose
  first sentence is "Output ONLY SQL — no prose", so a question is the one thing it cannot carry
  out. Given an instruction it cannot obey beside one it can, it obeyed the one it could and
  invented `orders(placed_at)`, and the invented SQL landed at the caret with nothing on screen
  marking it as ungrounded (`the_withheld_schema_note_is_something_a_one_shot_can_do`). The chat
  panel's wording is right *there*, where the model can answer back.
  `render_ai_context` also tells the model how a **schema change** reaches the
  user — call `propose_table_change`, then echo the JSON in a `FENCE_TAG` block — spelled out in
  the prompt as well as in the tool's own description, because the shape it replaces (an `ALTER` in
  a code block for the user to run) is the one every model reaches for by default. It is stated
  whether or not queries are allowed, since proposing reads the schema and that was never the gated
  part, and the tag comes from the constant the renderer reads
  (`the_prompt_names_the_fence_tag_the_renderer_reads`). The MCP server itself is `mcp.rs` — four tools (`run_query`, `list_schema`,
  `describe_table`, `propose_table_change`), described for **this** connection's engine and listed
  for **this** connection's access level: `tools_list(engine, reads_data)` builds
  `run_query`'s advertised statement heads from `schemaic_core::sql::read_only_heads`, the same list
  the gate enforces, and names the engine with `SqlDialect::engine_label()`. A hard-coded
  `SELECT/SHOW/DESCRIBE/EXPLAIN/WITH` told every model that a SQLite connection accepted
  `SHOW TABLES`, so a model reasoning from the tool's own text spent turns on statements that could
  only come back as parser errors; `run_query_advertises_exactly_the_heads_its_gate_allows` walks
  every advertised head back through the gate. Below `AiData::Full` the tool is **withheld, not
  merely refused**: `reads_row_data` is the one predicate `tools_list` filters on and `refusal_for`
  turns a call away with (`NO_DATA_ACCESS`, which names the setting so the model asks the user for
  values instead of retrying). Listing `run_query` and denying the call is worse than not listing
  it — the CLI's `--allowedTools` withheld it while `tools/list` still advertised it, so the model
  planned a turn around a tool it could see, offered to run a query and analyse the results, and
  learned only after the user agreed that the call was denied. No system-prompt sentence outranks a
  tool the model can see, which is why the listing is where the level has to bite; the server-side
  refusal is the backstop for a client working from a stale listing. The mirror of that listing is
  `ai.rs`'s `AI_TOOLS_WITH_QUERY` / `AI_TOOLS_READ_ONLY`, the `--allowedTools` the CLI is spawned
  with: it runs non-interactively, so a tool missing from its level's list has nobody to approve it
  and the call is simply denied. Two lists in two files drift —
  `propose_table_change` was offered by the server from the day it landed and named by neither
  list, so the assistant's check of a proposed change against the live table was denied every time,
  invisibly, since the model then writes the fenced block from the schema it already has and the
  user sees a preview either way. `every_offered_tool_is_allow_listed_at_its_level` holds the two
  sides equal per engine and per level. Everything dialect-shaped here reads `dialect_of` →
  `Engine::dialect()`, whose match is exhaustive, and so does `main.rs`'s namesake (via
  `dialect_for`). Both were `if engine == Postgres { Postgres } else { MySql }`, which compiled
  cleanly when SQLite arrived and sorted it onto the MySQL side: the read-only gate lexed AI-issued
  SQL by MySQL's backslash-escape and `#`-comment rules, neither of which SQLite has, and
  `describe_table`'s sample query took `filter::table_query`'s MySQL branch — qualified `main.t`,
  which is the SQLite name the surrounding entry says never to emit, and checked the name against
  MySQL's reserved words rather than `SQLITE_RESERVED`, so a table named `isnull`, `notnull`,
  `returning` or `transaction` (reserved in SQLite, not in MySQL) came out unquoted and would not
  parse (`the_dialect_is_the_engines_own_for_every_engine`). The DDL paths escaped by luck alone:
  `TableInfo::create_ddl` hands back SQLite's own `create_sql` for a real table without consulting
  the dialect, and a **view** fell through to `ddl::view_ddl`'s MySQL shape, which was only cosmetic
  because SQLite accepts backticks and its views carry no `view_options`. `secrets.rs` is the
  keyring-backed `SecretStore` behind `core::secrets`.
  `propose_table_change` is the odd one out and stays read-only like the rest: it takes a
  `core::propose::Proposal`, introspects the table, runs `propose::apply` → `ddl::diff` → `emit`,
  and hands the model back the change list in the *preview's own words* plus the SQL and anything
  destructive in it — **it executes nothing**. What it buys is a self-correction loop: the model
  learns here that it misread a column name, rather than the user being handed an offer that can't
  be applied. It cannot reach the user directly — this is a separate `--mcp-serve` process with no
  route into the app's overlays — so a proposal arrives only when the model echoes the same JSON
  into its reply in a `propose::FENCE_TAG` block, which `ui::markdown` picks up through
  `is_proposal_tag`. A model that skips the tool has therefore lost a *check*, not a safety rail:
  the preview and the Apply click sit downstream of both paths.
  `proposal_from_args` strips the tool's own `database` argument before parsing, because `Proposal`
  is `deny_unknown_fields` on purpose — an invented key has to fail loudly, since a silently-dropped
  one is a change the user was promised and wouldn't get. The tag is advertised from the constant
  the app extracts on (`the_proposal_tool_advertises_the_tag_the_app_looks_for`), so the two can't
  drift into a block nothing picks up.
  - `heap.rs` — process-wide heap accounting. `Tracking` is installed as the global allocator and
    adds only two atomics — **live** bytes (allocated − freed) and the running peak — over the
    system allocator. It exists to answer one question the OS can't: whether memory growth is a
    real leak or benign allocator/OS retention. Live returning to its baseline after a table closes
    while the working set stays high is the allocator holding freed pages for reuse; live *not*
    returning is the leak.
  - `logging.rs` — the tracing subscriber, and the log file the shipped app writes to. **The
    installed app used to log nowhere at all.** Release builds are GUI-subsystem on Windows
    (`windows_subsystem = "windows"`, in `main.rs`), so `tracing_subscriber`'s default stdout writer
    hands every line to a console that does not exist; when the first auto-update failure happened
    in the field — the header chip flashing "Updating…" and then vanishing, which is the
    deliberately-silent `UpdateState::Failed` — the error string was dropped on the floor and
    nothing on the machine recorded why. It was diagnosable at all only because Velopack keeps a
    separate log for its out-of-process `Update.exe`. So `init()` installs a **file** writer always,
    at `log_path()` — `%APPDATA%/schemaic/schemaic.log`, or the platform equivalent — in the config
    directory beside `tabs.json` and the rest of the state, so there is one directory to point a
    user at; that is what `core::persist::config_dir` is `pub` for. Debug builds tee to stdout on
    top of it (`MakeWriterExt::and`). ANSI is off, because escape codes are noise in a file and the
    file is the writer that always exists — a debug console losing its colour is the price of the
    two agreeing. **`DEFAULT_FILTER` is `"schemaic=info,velopack=info"`, and the `velopack` half is
    load-bearing rather than decoration.** Velopack's *in-process* half (`UpdateManager`) logs
    through the `log` crate, which `tracing-subscriber` already bridges into `tracing`, but the old
    `schemaic=info` filter discarded every one of those records because they carry a `velopack`
    target — that was the other half of why the field failure was undiagnosable
    (`the_default_filter_admits_velopack` pins it); `RUST_LOG` still overrides the pair where it is
    set. Rotation is a size check **once per launch**, not per write: `MAX_LOG_BYTES` is 4 MB and
    one generation is kept as `schemaic.log.1`, so the worst case on disk is twice that, and a
    `stat` stays out of the path of every trace call. Failing to open the log is not fatal — it
    degrades to the old stdout-only behaviour rather than refusing to start, since a read-only or
    missing config directory is a reason to lose logs, not the app. `FileWriter`/`FileHandle` are a
    hand-rolled `MakeWriter` over one `Arc<Mutex<File>>` rather than a dependency on
    `tracing-appender`: the whole requirement is "append to one file", and the rotation it needs is
    a startup size check rather than the time-based scheme that crate exists for. A poisoned lock
    drops the line instead of panicking a second time from inside the logger. **A panic went the
    same way, for the same reason, and worse:** Rust's default hook writes the payload to *stderr*,
    which on a GUI-subsystem release build is the same console that does not exist — so the one
    failure class that kills the process left no trace of itself at all. `install_panic_hook()`,
    called from `main` immediately after `init()` (before it, the report would be formatted and
    handed to a subscriber that does not exist yet), routes the payload, thread name, source
    location and a **forced** backtrace through `tracing::error!` and so into the same file.
    `Backtrace::force_capture`, not `capture`: the latter is governed by `RUST_BACKTRACE`, which
    nobody has set on the machine that just crashed a GUI app, and the cost only lands on a process
    that is already dying. The hook **chains** the one it replaced rather than replacing it
    outright, so a debug build (and a terminal launch on Linux) keeps its stderr message — this
    adds a destination, it does not take one away. `panic_report` and `payload_text` are split out
    as pure functions because the hook itself is process-global and cannot be driven from a test
    that is not itself panicking; `payload_text` downcasts to both `&str` and `String` because
    `panic!("{x}")` boxes the latter and a `&str`-only downcast would lose most panics.
    `an_installed_hook_writes_the_panic_through_tracing` guards the *seam* rather than the pieces —
    a thread-local subscriber over a capture buffer, a real `catch_unwind` panic — since a
    well-formed `panic_report` proves nothing if the hook never reaches a subscriber.
  - `update.rs` — the Velopack half of `core::update`: a background check at startup and every three
    hours after (`RECHECK_INTERVAL`), and the "Restart to update" action `start` hands back for the
    header chip. **Velopack's API is synchronous and does network + file I/O**, so every call into it
    runs under `handle.spawn_blocking` and comes back through Floem's async→UI seam —
    `create_ext_action` for the start-of-download and settle hops, `create_signal_from_channel` for
    progress. The forwarding thread between the two channels is not
    ceremony: Velopack reports progress on a `std::sync::mpsc::Sender<i16>` while
    `create_signal_from_channel` wants a crossbeam receiver and must be called on the UI thread, and
    `download_updates` blocks the worker for the whole download, so nothing on that thread is free to
    drain the ticks. **That whole progress path — the channel pair, the forwarding thread,
    `create_signal_from_channel` and the effect folding ticks in through `with_progress` — is built
    once per process in `start`, not per round.** `mpsc::Sender` is `Clone`, so each round hands
    Velopack its own handle; building it per round would leak a signal and an effect every interval
    for as long as the app stayed open, which is precisely the sort of thing an app left running for
    days would accumulate. The forwarder thread therefore parks on `recv` for the process lifetime,
    which is what keeping `velo_tx` alive in `start` buys.
    **Each settled round arms the next**, through `exec_after(RECHECK_INTERVAL, …)` and only when
    `core::update`'s `should_recheck` says another round could still change the answer — which is why
    `spawn_check`'s `finish` payload is `(CheckGate, Result<Option<VelopackAsset>, String>)` rather
    than the result alone: the settle handler has to know whether the gate allowed the round. The
    re-arm goes through a `Recheck` handle (`Rc<RefCell<Option<Rc<dyn Fn()>>>>`), the same
    self-holding-closure shape the terminal cursor-blink tick and `main.rs`'s
    `start_resource_monitor` use, because the closure has to be able to schedule *itself*; `start`
    clones the closure out of the cell before calling it, since that first call re-arms through the
    same cell and would otherwise run under a live `borrow()`. The timer bails when
    `state.try_get_untracked()` is `None`, following the *Floem 0.2 gotchas* rule for perpetual
    self-rescheduling ticks: the signal is disposed at shutdown and a surviving timer would read
    freed memory. The feed is a `GithubSource` on `https://github.com/fadion/schemaic`, read
    anonymously — 60 requests/hour per IP, and a round costs two requests (a releases listing and a
    ~760-byte manifest), so three-hourly polling stays three orders of magnitude clear of the limit.
    It is not shorter because nothing would be gained: the thing being waited for is a human tagging
    a release.
    `UpdateManager::new` *failing* is how "not a Velopack install" is detected, and is what feeds
    `check_gate`; a downgrade (`UpdateInfo::IsDowngrade` — a yanked release, or a dev build ahead of
    the tag) is skipped rather than walking the user backwards silently. The apply action builds its
    `create_ext_action` per click because that returns an `FnOnce` and the chip stays on screen
    until the window actually goes.
    **The apply action calls `wait_exit_then_apply_updates` and then `floem::close_window`, never
    `apply_updates_and_restart`.** The restart-now call exits this process on the spot, which would
    skip the `WindowClosed` handler `flush_session` hangs off and lose up to one debounce interval of
    unsaved tab text — the same reasoning the caption bar's close button carries in
    `ui::window_chrome`. Closing the window runs the normal shutdown, and the updater we just handed
    off to is already sitting there waiting for this process to go away.
    **`FORCE_UPDATE_BADGE` is a development switch and shipping it `true` is a real hazard**, which
    is why a test now holds it down. It pins the state at `UpdateState::Ready` and skips the real
    check — the only way to look at the "Restart to update" chip while working on it, since every
    other route there needs two tagged releases and an update genuinely in flight. Left `true` in a
    commit it would show every user a permanent restart offer that does nothing (nothing is staged,
    so `apply_action` returns early), and nothing else in the tree reads it, so the build would be
    perfectly green. `the_forced_update_badge_is_off_in_a_committed_tree` is the part that notices;
    a `const _: () = assert!(…)` would catch it a step earlier but refuse to compile exactly when a
    developer has flipped it on purpose. The module's own *decisions* live in `core::update`
    (`check_gate`, `should_recheck`, `UpdateState::with_progress`) and are tested there — what is
    left here is orchestration that needs a real install and a real feed, plus the constants, so
    the constants are what the local tests pin: the switch, the published `OPT_OUT_VAR` name, the
    feed URL, and `RECHECK_INTERVAL` against the anonymous rate limit.
    **Every leg packs on an explicit, non-default Velopack channel — `win-x64`, `linux-x64`,
    `osx-arm64` — and those three strings are effectively permanent.** Left to default the channel
    would be plain `win` and `linux`, and a *default* channel reaches only the manifest name: both
    platforms would then emit a package called `Schemaic-<version>-full.nupkg`, which two jobs
    cannot both upload to one GitHub Release. The rejected upload is the mild failure.
    `GithubSource::download_release_entry` resolves a package by matching its `FileName` across the
    release's assets, so a Linux client following `releases.linux.json` would be handed whichever
    asset won that name — plausibly the Windows build. Naming a non-default channel puts it into
    every file name instead, which is what keeps the two sets apart: measured on 2026-08-19,
    `--channel win-x64` produced `Schemaic-0.16.0-win-x64-full.nupkg`, `Schemaic-win-x64-Setup.exe`
    and `releases.win-x64.json`, against `Schemaic-0.16.0-linux-x64-full.nupkg`,
    `Schemaic-linux-x64.AppImage` and `releases.linux-x64.json` for `--channel linux-x64`.
    **There is no app-side counterpart and none is needed**: the channel is baked into the manifest
    of the release an app was packed from, so an installed app already asks for the channel it came
    from. `update.rs` passes `None` for `UpdateManager::new`'s whole `UpdateOptions`, which leaves
    `ExplicitChannel` unset — that field is for *switching* channels, not for declaring the one you
    were built on. That is also what makes the names unchangeable afterwards, which is stated as an
    invariant below.
    **Releases carry full packages only — `release.yml` passes `--delta None` on both platforms —
    and that is a correctness choice, not a bandwidth one.** Deltas broke the *second* consecutive
    update. A client that reaches version N by applying a delta ends up holding a **locally
    reassembled** package for N rather than the one CI built: v0.16.1's package was 23,606,057 bytes
    on the release and 23,605,881 on disk after reassembly, with a different SHA1. The delta for
    N+1 is computed against CI's copy, so it is applied to a base the build never saw. v0.16.0 →
    v0.16.1 worked only because that base had arrived whole from `Setup.exe`; v0.16.1 → v0.16.2
    failed in the field, and failed *silently*, because that is what `UpdateState::Failed` does. A
    full package always verifies against the manifest whatever route the client took to get where it
    is. The cost is roughly 15 MB per update on Windows, and next to nothing on Linux, where the
    delta was saving 16% against an already-compressed AppImage.
    **The Linux `.deb` and `.rpm` sit deliberately outside all of the above.** `release.yml` wraps
    the same zigbuild binary the tarball carries with `packaging/linux/` — `stage-payload.sh` lays
    out the payload once so the two formats cannot drift, `build-deb.sh` and `build-rpm.sh` +
    `schemaic.spec` add each format's metadata, and `install.sh` at the repo root picks between
    them and the AppImage. Those installs land in `/usr/bin`, which is *not* a Velopack install, so
    `UpdateManager::new` fails, `check_gate` answers `NotInstalled` and `should_recheck` ends the
    poll loop for good — the correct outcome (a distribution package is the package manager's to
    update), and the reason the AppImage stays the recommended Linux artifact.
    **Both packages hand-write their dependency lists, and no scanner can replace them**: `readelf
    -d` on the binary lists glibc and nothing else, because winit reaches X11, Wayland and xkbcommon
    through `libloading` and wgpu reaches Vulkan and EGL the same way. An automatically derived list
    therefore produces a package that installs cleanly and then cannot open a window. The two spell
    the same set differently — the `.deb` names Debian *packages* because dpkg has no soname
    provides, the spec names *sonames* because every rpm distribution auto-provides them, which is
    what lets one spec serve Fedora, RHEL and openSUSE despite their disagreeing on nearly every
    package name.
    **macOS is a third leg on its own `osx-arm64` channel — permanent for the reason above, and
    named `arm64` rather than a bare `osx` so that an Intel leg can be added later without renaming
    this one — and Velopack builds its bundle for us.** `release.yml` hands `vpk pack` a staged
    `dist/` and it produces `Schemaic.app`, a `Schemaic-osx-arm64-Setup.pkg` and a ditto-zipped
    `-Portable.zip` of the bundle — which is why macOS is the one leg that publishes no archive of
    its own, and why the `--noPortable` Windows passes is absent there. **That zip is built but not published**: it is the only predictable
    handle on the finished bundle, and a `hdiutil` step unpacks it into the `.dmg` that *is*
    published, because the zip and the `.dmg` are the same route and only one of them is the one
    Mac users recognise. **A `.app` dragged from that `.dmg` self-updates exactly as the `.pkg`'s
    does** — Velopack's `OsxVelopackLocator` works from "am I inside a `.app`" and finds `UpdateMac`
    in `Contents/MacOS`, caring nothing for how the bundle arrived. The `.icns` `vpk` insists on (it validates the extension) is generated on
    the runner from `assets/icon-1024.png` with `sips` and `iconutil`, so the PNG stays the single
    icon source. Unlike a `.deb`, a `.pkg` install **is** a Velopack install, so the in-app update
    check runs there exactly as it does on Windows. It is unsigned and un-notarized by the same
    decision the Windows installer carries.
    **That decision is to ship unsigned, and deliberately *not* self-signed** (taken 2026-08-19).
    The app is open source with no audience yet, so neither an application to the SignPath
    Foundation nor a paid certificate earns its cost today — but the part worth writing down is that
    self-signing is not the cheap fallback it looks like. A self-signed Authenticode certificate
    chains to no root any user trusts, so SmartScreen and "Windows protected your PC" say *Unknown
    publisher* exactly as they do for an unsigned binary, and no reputation accrues: SmartScreen
    reputation attaches to a certificate chaining to a *trusted* root, plus the file hash. Some AV
    heuristics score an untrusted signature worse than none. On Linux the question does not arise in
    that form at all — there is no CA-based code signing, only GPG with your own key, self-signed by
    construction, and nothing on the path we ship verifies it: no runtime or desktop checks an
    AppImage's embedded signature by default, and Velopack does not sign AppImages at all. GPG
    becomes mandatory only if we ever run our own apt repository, since apt refuses an unsigned
    `Release`.
    **Signing is not a prerequisite for auto-update**, and an earlier version of this reasoning
    wrongly made it one, on the grounds that auto-update pushes executables. Velopack's update
    integrity comes from HTTPS plus a size/hash check against `releases.<channel>.json`
    (`ChecksumFailedException`); nothing in its documented flow verifies an Authenticode signature on
    a downloaded package. Authenticode's real value here is the *first-install* SmartScreen
    experience and detecting a locally tampered file — polish, not correctness. What we eat for it:
    `Setup.exe` shows the unknown-publisher warning on first download, identical to the raw `.zip` it
    replaced and so not a regression, and Squirrel-family installer stubs have a history of AV false
    positives that being unsigned makes likelier — which lands on `Update.exe`, the one that runs on
    every update. Worth watching once there are real users.
    **When we do sign, the hook is one `vpk pack` argument, not a redesign** — `--signParams` (short
    `-n`) or `--signTemplate`, which signs the app *and* Velopack's `Update.exe`/`Setup.exe` stubs —
    which is why this decision blocks nothing. The options on file: signpath.org, the Foundation, is
    free for open source (not signpath.io, the paid enterprise product it runs on), and a licence
    check on 2026-07-26 found us eligible — MIT, OSI-approved, no proprietary components — at the
    cost of an application, staying actively maintained, and a publisher string reading "SignPath
    Foundation" rather than ours; the flow would be `signpath/github-action-submit-signing-request`,
    packing first, then submitting, then attaching the signed result to the Release, with the key
    never touching our runner. Certum's Open Source Code Signing certificate (~€70–100/yr,
    cloud-signable in CI, in *our* name) is the paid alternative. Azure Trusted Signing (~$120/yr) is
    out regardless: since 2025-04-02 its onboarding admits only US/Canada organisations with three
    years of history, no individuals and no EU enrolment. All of these are OV, and **no OV
    certificate grants instant SmartScreen trust** — reputation still builds with download volume, so
    signing buys a better first-run dialog rather than a silent one.
    **`release.yml` also answers to `workflow_dispatch`, with every publish step gated on
    `github.ref_type == 'tag'`.** A tag used to be the only way to run the file, which made a
    packaging mistake discoverable only once the tag existed and was awkward to retract — the macOS
    leg was proven this way before it ever saw a version number. The dry run additionally skips the
    feed-history fetch: without a tag the version falls back to the workspace one, and for any
    channel that has already shipped that is a version `vpk pack` refuses to pack over.
    **`main.rs` runs `velopack::VelopackApp::build().run()` near the top of `main`**, before tracing,
    the font registration, the tokio runtime and any Floem signal or `Scope`: the installer and the
    updater re-invoke the exe with `--veloapp-*` args, and `run()` services those and then
    *terminates the process*, so anything set up ahead of it is either built to be thrown away or
    half-initialised when the process dies mid-hook. With no hook args it returns immediately, so a
    normal launch pays nothing. It sits deliberately *after* the `--mcp-serve` early exit, which is a
    different program — a stdio JSON-RPC server whose stdout is the protocol stream, so nothing may
    write to stdout ahead of it. The two flag sets never co-occur (one comes from the installer, the
    other from the `claude` CLI), so the ordering between them is free, and this way the protocol
    stream stays clean.

## Architecture invariants (don't regress these)

Re-introducing the anti-patterns these guard against is a regression:

- **The write guard lives on the run action, not in a caller of it.** Every path that executes
  user SQL goes through `TabsActions::run`/`run_all`, which *are* the guarded pair: they call
  `schemaic_core::sql::run_verdict` (pure + tested) and, on anything but `Allow`, park the request
  in `ui.overlay.run_guard` and execute **nothing**. The unguarded actions never leave
  `schemaic-app`; `TabsActions::run_anyway` is the only way back to them, and it replays only what
  the guard parked. The editor pane renders the bar — it does not own the guard.
  This is written down because the guard used to be two closures inside `editor_pane.rs`'s *view
  body*, so it protected exactly one caller: the command palette's `>run` and the AI chat's
  **Insert & Run** both reached the raw action and ran writes past all three protections — the
  missing-`WHERE` net, `confirm_writes`, and the read-only-connection block that by design has no
  "Run anyway". **Don't add a run path that takes the raw action, and don't re-implement the
  verdict** — a new protection is an arm of `run_verdict`, and a `RunVerdict::Block` must stay
  un-overridable. (`plan_view`'s `contains_write` is not a second guard: it decides whether
  `EXPLAIN ANALYZE` may run a statement for its timings.)
- **A row reaches the model only where `AiData` says it may, and the level is the connection's.**
  Every path that can put a cell value in a prompt — `run_query`, `describe_table`'s samples, the
  grid's attach-to-chat, AI Summary, AI Fill, AI Seed — is gated on
  `connection::AiData::{may_query, may_attach}`, resolved from **the connection the data came
  from** (`grid::ai_data_of` reads the result's `conn_id`, not the active one). Don't add a path
  that samples, quotes or forwards result values without asking, and don't answer the question
  from a new flag: three unrelated toggles is precisely how a user comes to believe they are
  protected while a fourth path ships samples anyway — which is the state this replaced, where the
  AI panel's global "run queries" switch left `describe_table`'s five sample rows, the cell
  summary's column sample and Seed Table's bottom sample all flowing regardless. Gate at the
  action as well as at the menu: a menu built a moment before the connection was locked down is
  still on screen. The default (`OnRequest`) grants **no** automatic access — rows go only where
  the user attached them, so the gesture is the consent and there is no setting to forget. And
  what is sent is kept honest at both ends: `result_shape` states out loud that no rows were sent,
  and `result_attachment` states the cap it applied.
- **One SQL boundary lexer.** Any code scanning SQL for string / `-- ` / `#` / `/* */` / backtick /
  `$tag$` boundaries MUST build on `schemaic_core::sql::skip_noncode` (statement split, WHERE guard, AI
  read-only gate, `intel`'s tokenizer, `sql_highlight`, `sqlfmt`). Never hand-roll a second
  scanner — five drifting copies was the original bug. **It's dialect-aware:** `skip_noncode`/
  `skip_comment` (and the `sql.rs` helpers built on them — `statement_bounds`/`ranges`/`range`,
  `read_only_reason`, `has_top_level_where`/`unsafe_reason`/`first_unsafe`/`contains_write`) take a
  `SqlDialect`, so pass the connection's dialect (Postgres `#` is an operator not a comment, `$tag$…$tag$`
  is a string, `"…"` an identifier, `\`-escapes only in MySQL / PG `E'…'`; SQLite takes `"x"`,
  `` `x` `` *and* `[x]` as identifiers and has no backslash escape at all). **It is dialect-aware
  about statement *boundaries* too, not only tokens:** on SQLite a `;` inside a `CREATE TRIGGER`'s
  `BEGIN … END` block does not end the statement. SQLite has no `DELIMITER` directive to hide those
  semicolons behind, so without this `statement_bounds` cut a trigger in half and Run Everything
  sent `… BEGIN UPDATE log SET n = 1;` and `END;` as two statements. A private `TriggerScan` state
  machine counts block openers (`BEGIN`, `CASE`) against `END`, so a `CASE … END` inside the body
  can't end it early — the same thing `sqlite3_complete()` does for SQLite's own shell. MySQL and
  PostgreSQL boundaries are deliberately untouched: MySQL's trigger bodies go behind `DELIMITER`,
  and changing that would silently alter what Run Everything sends a server. The other half of that
  is `ddl::client_script` — what `ChangeSet::editor_script` and a routine's `Generate DDL` both go
  through — which asks `== MySql` before reaching for `DELIMITER $$` so a SQLite plan is never
  handed a directive the engine has never heard of, and **terminates every statement** on the way
  past. That second rule reads as a no-op because nearly everything already ends in its own `;`;
  the exception is a MySQL routine's `CREATE`, which deliberately carries none (the apply path
  sends each step whole) and whose body is full of `;` — so two of them joined for a reader ran
  together, and one pasted into a query tab was cut mid-body. It is one function rather than two
  because that is exactly the divergence that produced the bug: `editor_script` knew the rule and
  the schema tree's copy path didn't. **Ask the capability,
  never the engine** — the rules are predicates on `SqlDialect` (see `sql.rs` above), because
  `dialect == Postgres` / `!= MySql` compiles cleanly while silently sorting a third engine onto
  whichever side it falls, and two of the answers it got wrong for SQLite could hide a `WHERE`
  from the guard. It is the same rule away from the lexer: the table designer's form asked
  `!= Postgres` in three places and thereby offered a SQLite table a storage engine, a table
  collation, comments and `ON UPDATE`, none of which that engine has — each now asks for the
  capability. **Deriving the dialect is the same question**: `Engine::dialect()` is the one
  exhaustive answer, and the hand-written `if engine == Postgres { Postgres } else { MySql }` that
  stood in for it in `app::mcp` and `app::main` is exactly how SQLite came to be lexed by MySQL's
  rules on the AI path (see `schemaic-app` below). The **terminal's DB-client button** was the last
  of that shape to fall (`open_db_cli`): `if is_postgres { psql } else { mysql }` handed a SQLite
  connection to the MySQL client along with the inert `127.0.0.1:3306` a *file* connection carries,
  so the button either reported no client or opened a session against an unrelated local server and
  badged it with this connection's name. It is a `match` on `Engine` now, with `sqlite_shell` as the
  third arm — **native `PATH` only, no WSL fallback**, because a host and port mean the same thing on
  both sides of that boundary and a *path* does not: `sqlite3 'C:\data\app.db'` inside WSL doesn't
  fail, it creates an empty database under that literal name. The same arm is why `wrap_launcher`
  takes `Option<(var, password)>` — a client with no credential sets no variable and names none in
  `WSLENV`, which is not the same as passing an empty password. **No exceptions** —
  `intel::tokenize_range` (the mid-edit byte-position *fallback*) is dialect-aware too, and so are the
  `intel` entry points that reach it (`clause_context`/`clause_continuation`/`join_targets`/
  `expand_star`/`signature_help` all take a `SqlDialect`). It additionally lifts a **quoted identifier**
  out as a word — `` `t` `` on MySQL, `"t"` on PG, any of the three on SQLite — since that's the form
  Schemaic itself generates and the fallback is exactly what runs mid-`WHERE`; `intel::ident_quote`
  answers per *byte* rather than holding one quote char, because SQLite's `[` doesn't even close
  with the byte it opened with.
- **Structure-aware SQL analysis goes through `schemaic_core::intel` (real per-dialect AST), not new
  hand-rolled scanners.** Scope resolution, completion context, and diagnostics build on the
  `sqlparser` AST (with a `skip_noncode` fallback for mid-edit); the **DB stays the semantic
  authority** (don't hand-roll type checking / name resolution — that's a planned PREPARE/EXPLAIN
  tier). New dialects are a `SqlDialect` arm, not a parallel analyzer. Use the AST for classification
  and `skip_noncode` byte offsets for positions by default; the exception is the per-scope column
  resolver (`intel::colres`), which relies on sqlparser 0.62's per-identifier spans (accurate — verified)
  because per-occurrence positions can't come from the lexer. Name resolution here is deliberately
  conservative: an unenumerable source (unloaded/unknown table, `SELECT *` derived/CTE) is *open* so
  uncertainty never yields a false positive; the DB stays the authority for type checking.
  **Read AST identifiers unquoted** — `Ident`/`ObjectNamePart`'s `Display` re-adds the quoting, so a
  `` `t` ``/`"t"` name comes back quote-wrapped and never matches the catalog (which is keyed on bare
  names). Go through `intel::object_name_parts`, or `Ident::value` for a lone identifier.
- **One connection per operation — except a Manual-mode tab.** Every `Db` method opens a fresh
  connection, runs, and disconnects; that statelessness is why a dropped connection is never a
  problem. The *single* exception is manual-transaction mode: a tab set to `TxMode::Manual` pins one
  connection (`schemaic_db::Session`, one `Conn`/`Client` behind a `tokio::Mutex`) for the life of
  its transaction, held in the app's `sessions` map (tab id → `Arc<Session>`). Only the tab's own
  work routes there — run, Run Everything, grid writes, and the post-write re-fetch (which *must*,
  since no other connection can see uncommitted rows). Read-only side channels (schema
  introspection, live-validate `PREPARE`, EXPLAIN, Live Monitor, AI/MCP) stay on fresh connections so
  a long transaction can't block them. Don't add a second connection-caching path; extend `Session`.
  **SQLite has no exception at all** — every operation opens its own connection inside
  `spawn_blocking`, which is this invariant rather than a concession to a blocking driver, and
  `Session::open` refuses it (see `core::tx` above for why the pinned form needs its own design).
  In-transaction writes nest under a `SAVEPOINT` (`TxScope`) so the 1-row guard can roll back its own
  batch without ending the user's transaction, and the transaction *state* is the pure, tested
  `schemaic_core::tx::TxState` — engine divergence (PG poisons on error, MySQL implicitly commits on
  DDL) belongs there, not in UI conditionals.
  **Schema-editing DDL is the case that fits neither half** — it is the tab's own work *and* a write,
  so it takes the fresh connection and then queues there behind the lock the tab's own uncommitted
  `SELECT` holds. Two things keep that from being an unexplained hang, and a new write path off the
  session should ask both questions: the app raises a `TxPrompt` per `tx::ddl_blocking_tabs` (open
  transactions on the *connection* — Schemaic doesn't track which tables one has touched, so anything
  narrower would miss the case) before applying, and `run_ddl`'s connection carries
  `db::lock_wait_sql` so a lock nobody could ask about comes back as an error instead of never.
- **Connection identity is the `Db` handle / `conn_id`, never a `mysql://user:pass@host/db` URL.**
  Credentials go through `OptsBuilder`; never in a URL, argv, or log. The MCP subprocess gets its
  endpoint via a temp `--mcp-config` file, not argv. Don't add new plaintext-secret surfaces.
- **Connection secrets persist to the OS keyring, not `connections.json`.** DB/SSH passwords and the
  SSH key passphrase go through `schemaic_core::secrets` (`SecretStore` seam) + `schemaic-app`'s
  keyring-backed store; the JSON on disk is blanked and hydrated on load. All connection saves route
  through the app's `secrets::{load,save}_connections`/`forget_connection` (which wrap
  `persist::{load,save}_connections`) — never call `persist::save_connections` directly from the app,
  or you reintroduce plaintext. Plaintext in the JSON is a *fallback only* for a machine with no
  working keyring.
- **Own per-entity signals in a child `Scope`; dispose it *deferred*.** A `Tab`/`ConnNode` creates
  its signals in `parent.create_child()`; removal disposes that scope via
  `exec_after(Duration::ZERO, …)` — one tick later, after the keyed `dyn_container` has unmounted
  the old view. Synchronous disposal frees signals a still-mounted view reads this frame → panic.
  Same for any "replace + free" of scoped state.
- **Themable colors reach reactive styles as `fn() -> Color`, never a captured `Color`.** A `Color`
  read once at build freezes and won't follow a live theme switch; pass the fn and call it inside
  the `.style(move |s| …)` closure (see `FieldCfg::background`).
- **Pure logic lives in `schemaic-core` with unit tests** — SQL boundaries, edit-model analysis,
  export (incl. CSV formula-injection guard), diff, DDL. The UI keeps thin wrappers.
- **Generated DDL is never run silently, and never emitted from a second differ.** Every
  schema edit goes `TableDraft`/`ViewDraft` → `ddl::diff`/`diff_view` → `ChangeSet::emit` →
  the preview modal → `Db::run_ddl`. Don't add a path that builds `ALTER`/`CREATE`/`DROP` text somewhere else, and
  don't add one that applies a plan without the preview — the preview is where the destructive
  consequence is stated in plain language and where "Open in editor" hands the script over.
  **Nor is a plan applied in part**: what the dialect can't express is `ChangeSet::unsupported()`,
  the preview names each one and Apply refuses while it does — on the action, not only on the
  disabled button. Which gate to ask depends on the question: `supports_change` for a single change
  with no draft behind it, and for an editor the capability for *that object* —
  `supports_view_editing`, `supports_trigger_editing` or `supports_table_design`. All three answer
  true for every engine today (SQLite reaches a table edit by rebuilding, `Change::RebuildTable`),
  and all three **derive** that from `supports_change` rather than returning a literal, which is the
  only form of "always true" that isn't a constant with a function's name on it: the answer changes
  when the emitter's does, and a fourth engine gets whatever the change table says about it. A menu
  entry with **no** predicate is the same failure with nothing to grep for — the designer's three
  entries were exactly that until `supports_table_design` existed. **Keep asking them, and keep them
  apart**:
  they are per-object questions the menus ask per object, and what differs between engines has
  moved down to the narrower predicates that decide how an edit is *performed* rather than whether
  it is offered — `supports_or_replace_view` and `supports_view_rename`, both false on SQLite and
  only there. **What a per-object capability must not do is answer for an object it wasn't asked
  about**: `field_entries` and `key_entries` take `is_view` because the tree renders a column row
  under a view exactly as under a table, and a constant `true` there offered Edit column and a red
  Drop for something that has neither — opening the *table* designer on a view, whose every edit all
  three engines refuse. `table_designer::open_for_table` refuses a view outright as the second lock.
  Gate the menu entry on the right
  one rather than leaving the refusal to `Db::run_ddl`, which sees only strings. A
  new engine is a `SqlDialect` arm in `ddl.rs`'s emitter, not a parallel emitter. The
  round-trip gate (a draft built from a table must diff to *nothing*) is the test that keeps
  the introspected model and the emitter honest with each other; extend its fixtures when you
  widen the model.
- **Write-back is transactional with a 1-row safety net — and the *report* never claims more than
  the engine delivered.** `commit_writes` runs a `GridWrite`
  (DELETEs → UPDATEs → INSERTs) in one transaction, each statement required to affect exactly 1 row
  (else roll back all) — so an over-optimistic updatability analysis can't corrupt data. On SQLite
  the *analysis* is the part that has to be conservative, since no driver reports provenance there
  and it is derived from the statement (`intel::projection_of`, positional): anything but a plainly
  single-table `SELECT` is simply not editable. That set has grown by exactly one well-defined
  shape — items placed ahead of a lone *trailing* `*`, which is what makes `SELECT rowid, * FROM t`
  (a keyless table opened through its rowid) analysable — and by nothing else. The guard did not
  move with it: an implicit key is an ordinary key column to `commit_writes`, so the ordering and
  the 1-row net apply to it unchanged, and widening the analysis further is still the way this
  invariant gets regressed. **The net's premise is that a stale key matches zero rows, and an
  implicit key breaks it** — SQLite reassigns rowids, so a number the grid still holds can name a
  different row, and an `UPDATE` on it affects exactly the 1 the guard wants to see. That is
  repaired where the key is built rather than where it is checked: `EditTable::confirm_cols` puts
  the values the grid read into the same `WHERE` for an implicit key only, and `edit::row_key` is
  the one builder that appends them. Restoring the premise is the shape any future key of this kind
  has to take; loosening the guard is not. Its rollback, by contrast, is the one that needs no
  hedging — there is no non-transactional table type. That
  promise is MySQL-engine-dependent: `MyISAM`/`MEMORY`/`ARCHIVE`/`CSV` ignore `BEGIN`/`ROLLBACK`,
  and `ROLLBACK` *succeeds* there while raising warning 1196. So no write path may discard a
  rollback's outcome (`let _ = conn.query_drop("ROLLBACK")` was the bug): roll back through
  `rollback()`, which reads `SHOW WARNINGS`, and append `core::model::Rollback::note()` to the
  error. **A cancel is an exit like any other**: `Db::import_rows` used to `kill_query` and
  disconnect, and the modal then said "the transaction rolled back, so nothing was written" —
  which on those engines is false, so the user re-ran the import and doubled ~250k rows. It rolls
  back on the same connection now and reports what that achieved, `DbError::Cancelled` meaning the
  rollback completed and an incomplete one arriving as an error carrying the note.
  `one_row_verdict` states only what the guard saw — it runs *before* the rollback and can't
  know what it achieved, so **every** executor appends the clause once it does: SQLite's was the one
  that didn't, and a user reading *"UPDATE main.t affected 2 rows (expected exactly 1)"* with nothing
  after it had no way to know the other three staged edits went back too. Its import likewise takes
  `Rollback::Complete.note()` rather than a third hand-written wording of the sentence `Rollback`
  exists to unify. `engine_is_transactional` is the predicate (unknown ⇒ not transactional,
  same rule as `pg_replaceable`); the import modal warns from it before the load starts. Commits
  with inserts/deletes full-re-run the query (membership/order changed); pure-UPDATE commits splice
  in place. Both halves of that rule are **pure and tested in `core::model`**, and both engines'
  executors call them: `GridWrite::plan` is the statement order and `one_row_verdict` is the
  per-statement verdict *and* its message — so neither can drift between MySQL and PostgreSQL, and
  a change to `affected != 1` fails a test rather than passing silently.
- **A destructive modal action guards its own launch, in the same step that launches it.** Import,
  the DDL preview's Apply and Server Activity's kill are the three, and they go through
  `widgets::accept_launch(in_flight, read_only)` — not through the disabled button, which is what
  *says* the action is unavailable and takes effect on a later update pass. `run_import` set a busy
  flag and never read it, resting on a comment that "its Import button is disabled while one is in
  flight": true of the next pass and false within a single key dispatch, so one Space started
  **two** bulk loads of the same file, both committing, with the second launch overwriting the
  cancellation token so the first could no longer be stopped. A new destructive action asks the same function; a guard re-derived per site
  is one that will be derived differently.
  **`read_only` covers server administration, not only data writes** — an open question the
  codebase had never recorded, until the kill arrived and answered it by asking nothing at all. The
  flag is the protection with no "Run anyway", and terminating a live client session — rolling its
  transaction back under it — is the most destructive thing the app can do to a server it has been
  told not to write to. So the session row menu's two kill entries and the lock-wait banner's
  one-click terminate all ask at the click, through the app-side `may_launch_destructive` re-export
  of the same function, and a refusal says why in the panel's `kill_error` line rather than doing
  nothing.
- **One identifier quoter, as there is one boundary lexer.** Every path that quotes an identifier
  ends at `export::ident_sql` (unconditional — for SQL that is only executed) or its sibling
  `export::ident_if_needed` (only when a bare name would name something else — for SQL the user
  reads and edits). **How a table is *addressed* is the same rule once over**:
  `export::qualified_table` — MySQL qualifies with the database (its connection is server-level),
  PostgreSQL with the namespace, and SQLite names it **bare**, since a connection *is* one file and
  `main` is SQLite's word for "the file you opened" rather than a name the user chose, so
  `INSERT INTO "main"."t"` is noise on every exported row and wrong the moment that SQL is pasted
  where another file is attached under that name. `import::build_insert` held a second copy of it,
  which is how the SQLite case reached one path and not the other.
  `filter::quote_ident`, `schema::ddl_ident_in`, `db::pg::pg_ident`,
  `db::ident_sqlite` and
  `db::ident` are all thin delegations; the three engine-fixed ones in `schemaic-db` are bound by a
  test in that crate, since they can't take a dialect. **Don't write a fifth** — there were four,
  each having independently arrived at the same escaping, which is the drift hazard rather than the
  reassurance: the literal half of the same split (`schema::ddl_string` missing MySQL's backslash
  escaping while `export::sql_literal` had it) shipped as a High.
  SQLite *reads* three quotings but **emits only `"x"`**: it is the one of the three with a defined
  escape, since a `]` cannot be written inside brackets at all. Its literals take Postgres' arm —
  no backslash escape, so doubling one would corrupt the value.
  **The other half of a *conditional* quoter is which predicate the condition asks**, and that is
  the same bug by a second route: `ident_if_needed` and `filter::needs_quoting` ask
  `intel::must_quote_ident` (can this be a bare **identifier**), never `intel::is_reserved_word`
  (can this be a bare **alias**) — a diagnostic's question, answered by a deliberately laxer list.
  They asked the alias set, and on SQLite `CAST`, `IF` and `RAISE` sit in the gap, so a table named
  for one of them produced an `ORDER BY` that would not parse. Right quoter, wrong question. See
  `core::intel` for the measurement and the test that holds both lists to the engine itself.
- **Every schema-search surface matches through one predicate.** The schema tree's filter box and
  the Find-Anywhere palette answer the same question over the same `DbSchema`, so they go through
  `schema::TableInfo::matches_search` (name or any column) and `schema::ObjectItem::matches_search`
  (name only — a `detail()` match would surface a sequence because some unrelated table's name
  appeared in its owner). They were two predicates and the palette's simply **had no object arm**,
  so on a PostgreSQL connection Ctrl+P for a type you were looking at in the sidebar returned
  nothing. **The name-versus-term rule underneath all of them is `schema::object_name_matches`** —
  the ER diagram's find bar (`erd::search`) is the third surface and is a *caller*, not a fourth
  spelling, and `TableInfo::matches_search`/`any_column_matches` were folded onto it rather than
  each keeping its own `to_lowercase().contains`. The empty needle is why that matters beyond
  tidiness: the predicate owns the rule that an empty term matches nothing (every caller answers "no
  filter" separately), and while the callers spelled the comparison themselves that case was handled
  in some of them and not others. `overlays::schema_hits` is the palette's half split out as plain data for exactly this
  reason, and `overlays::find_tests` asserts the two surfaces return the same objects for a set of
  terms. **One deliberate divergence, and it is the only one allowed without a test change:** an
  *internal* object (an identity column's counter) is listed by the tree and withheld by the
  palette — a tree row is context, sitting under the table that owns it, while a palette row is a
  destination, and this one's activation is an editor that refuses to open. That withholding is
  `overlays::is_palette_target`, which **delegates to `object_editor::is_editable_object`** rather
  than re-spelling it, and **both** producers ask it — the live search *and* `lookup_object` on the
  search-history path. A `serial`'s sequence is an ordinary object and a legitimate result, so it
  can be remembered; migrating its column to an identity column makes that same sequence internal,
  and only the second gate stops the remembered row from opening an editor the server would refuse.
  Every path to `open_for_object` is gated (the tree's two, and this one) — don't add an ungated one.
- **A Find-Anywhere database is searched in three passes: table/view *names*, then objects, then
  columns** (`overlays::schema_hits`), and the object pass sits in the middle **deliberately**.
  Columns are the category that floods — one `user_id` foreign key across a hundred tables is a
  hundred hits — so with the objects appended last they were pushed past the 80-result cap
  entirely, and a type could not be found however precisely you typed its name. That is the same
  symptom the object arm was added to fix, arriving by a different route, which is why
  `an_object_survives_a_flood_of_column_matches` pins it with a deliberately small cap. Names are
  few, so the ordering costs only the tail of a broad column search. Two residual limits, both
  pre-existing and both accepted: a database with `limit` *table-name* matches can still starve its
  objects (a name match is rare where a column match is not), and the cap is **global across
  databases**, so a wide first database still contributes everything and later ones nothing.
- **The Find-Anywhere query is not debounced.** Unlike the schema tree's filter box
  (`debounced(filter_input, SEARCH_DEBOUNCE_MS)`), the palette's effect re-runs on every keystroke,
  over every loaded database. So anything it calls per-database is on a per-character path: that is
  why the object arm goes through `DbSchema::objects_matching` (clones only the hits) rather than
  `objects_all` (clones the database's every enum, domain and sequence, an enum's whole value list
  included, to answer a substring test). Same rule as `SignalGet::with` over `get`.
- **Identifier scanning treats bytes `>= 0x80` as word bytes** so Unicode identifiers tokenize whole.
  `sql::is_word_byte` (continues a word) and `sql::is_word_start` (begins one — `alphabetic`, since a
  digit can't start a name) are the **only** definitions, next to `skip_noncode` because they answer
  the other half of the same question: that one says where a token can't start, these say how far it
  runs. Don't inline the predicate at a new scanner — there were four copies of each, no test
  comparing them, and the rule had already been regressed and repaired once. `sql.rs` tests both over
  all 256 byte values and asserts they differ on exactly the digits.
- **A Velopack channel name is app identity, like `--packId`: add a name, never rename one.** The
  three `release.yml` packs with — `win-x64`, `linux-x64`, `osx-arm64` — are explicit because a
  *default* channel (`win`, `linux`) reaches only the manifest name, so both platforms emit one
  `Schemaic-<version>-full.nupkg`, and `GithubSource::download_release_entry` resolves a package by
  matching `FileName` across the release's assets: a Linux client following its own manifest can be
  handed the Windows build. Renaming one afterwards is the worse half, because an installed app asks
  for the channel baked into the manifest it was packed from — a rename orphans every install
  carrying the old name, silently, and there is no route back to those users to tell them. So a
  fourth platform gets a fourth name (`osx-x64` beside `osx-arm64`), never a widening of an existing
  one. **The guard is a CI step rather than a unit test**, which is this section's one standing
  exception to *test-enforced where possible*: no crate names a channel — `update.rs` passes `None`
  for the whole `UpdateOptions`, so not even `ExplicitChannel` mentions one — and `cargo test
  --workspace` therefore cannot see these strings at all. `release.yml`'s second step, right after
  `checkout` and ahead of the build so a bad name costs seconds instead of a compile, fails the job
  unless `matrix.channel` is one of the three. That catches the realistic mistake — a typo, or a
  rename of a shipped channel in the matrix — in seconds and in the same file as the mistake. It
  **cannot** catch someone editing the matrix and the step's allowlist together, and nothing inside
  the repository could: the guard pins the workflow to its own list, not to what has actually
  shipped on GitHub. The friction is the value — a rename has to be deliberate in two places, one of
  which is a comment explaining why it must not happen. Adding a platform is expected and safe: add
  the name in both places. See `app::update` for the packed file names this was measured against.
- **Splitting `lib.rs` / `main.rs`:** grep the line range for interleaved unrelated `fn`s first; a
  helper still used by code that stays goes to `widgets.rs` (glob-imported), not the new leaf
  module; mark cross-called items `pub(crate)`; build + `cargo fmt` + smoke-launch each step.

## UI conventions

- **No pointer cursor on buttons/icons** — native apps keep the arrow cursor; a pointer feels
  web-like. Use the default; reserve `CursorStyle::Text` for text inputs (a genuine hyperlink may
  keep `Pointer`).
- **Labels aren't selectable** — Floem's `Selectable` defaults to *true*, so every caption/header/tree
  row would drag-highlight like a web page. The workspace root sets `.class(LabelClass, |s|
  s.selectable(false))`, which cascades to the whole tree (and, via the captured context style, into
  dropdown popups); `tooltip_style` repeats it because a tip overlay only inherits `TooltipClass`.
  Text selection belongs to real text surfaces — `text_input`/`edit_field`, the SQL editor, and the
  terminal (which paints its own selection). Don't re-enable it on a label.
- **Colors live in `theme.rs`** as named fns — add one rather than inlining a hex literal. They read
  the *active* theme from `themes.rs` (reactive), so calling one inside a `.style(…)` closure follows
  a live theme switch for free.
  - **Reuse a theme color only when the shared scope makes sense** (the two spots are the same role and
    should always change together). Otherwise extract a separate named fn — even if it starts at the
    same hex — so each can be retuned independently (e.g. `seed_button()` alongside the identical
    `status_ok()`).
- **Theming (`themes.rs`)**: two independent axes — `UiTheme` (chrome: dark/light) and `EditorTheme`
  (editor surface + syntax tokens: One Dark Pro / Tokyo Night / Catppuccin Latte). A theme is a flat
  struct of named colour roles (hex). Active themes live in `Scope`-owned global `RwSignal`s;
  `theme::set_ui`/`set_editor` swap them. The choice is persisted (`ui_theme`/`editor_theme` in
  `UiState`) and seeded via `theme::init` before the view builds. Editor tokens re-highlight on
  switch because `SqlStyling::id()` returns `theme::editor_generation()`.
  - **Live-switch caveat**: a colour read *inside* a reactive `.style` closure updates instantly; one
    captured *by value* freezes at build time. Prefer `fn() -> Color` for anything themable (see
    `FieldCfg::background`).
- **Reactive text**: use `dyn_container` (no `floem::views::label`).
- **There is no accessibility tree, and there is nothing in this repository that can add one.**
  Floem 0.2.0 ships no AccessKit integration and no a11y surface of any kind — grepping the crate
  for `accesskit`/`accessibility`/`a11y` turns up only the English word in two doc comments about
  platform config — so nothing the app builds is exposed to Narrator, VoiceOver or Orca, whatever
  it is labelled. What the app *does* have is keyboard operability, and that is the axis worth
  spending on: `FocusRing`, `focus_root_with_ring`, spaced tab indices, `widgets::accept_launch`,
  and the `shortcuts.rs` table with the test that fails when a binding has no row. The README says
  this out loud under *Accessibility* rather than leaving someone to discover it after the
  download; revisit if a later Floem grows the layer.

## Floem 0.2 gotchas (learned the hard way)

- **One `on_scroll` per scroll** — setting it twice clobbers. `autohide` sets its own; a scroll
  needing custom `on_scroll` must inline `autohide_state()` (results grid + AI convo).
- **No `opacity` property.** Fade via color alpha (`multiply_alpha`) + `.transition_*`. Toggle
  visibility with `.hide()`/`.flex()` (display none/flex).
- **Inherited color doesn't animate to a child.** A parent's `.transition_color()` won't fade a
  child svg's `currentColor` — set color + transition on the element itself.
- **Style precedence**: a view's own (direct) style beats ancestor class styles; nearest ancestor
  class wins. Nest class overrides accordingly (dropdown popup restyle nests under `ListClass`).
- **`DoubleClick` consumes the second `PointerUp`** — clear drag/press state in the double-click
  handler too, not only in `PointerUp`.
- **A child that overflows *left* or *up* of its parent is painted but never hit-tested.** Floem
  hit-tests a subtree through `EventCx::should_send` (`floem-0.2.0/src/context.rs`), which builds the
  rect it tests as `id.layout_rect().with_origin(layout.location)` — it takes the **size** of the
  union of the view and its children (window coords, `compute_view_layout`) but re-anchors it at the
  view's **own** origin. So an overflowing child grows the parent's hit area rightward and downward
  only: a child placed at a negative offset is inside the union that produced the size, and outside
  the rectangle that gets tested. It is `continue`d past, and the pointer reaches whatever sits
  underneath instead. Nothing about this is visible in paint, which doesn't consult `should_send` —
  the thing renders perfectly and simply doesn't answer the mouse, hover states included, so the
  failure reads as "this view has no event handling" rather than as a placement bug.
  **The fix is to hoist the overflowing view to a layer whose own box already contains it**, not to
  stop flipping — off-screen is not better than unclickable. Menu submenus do exactly that
  (`widgets::submenu_layer`, last in the root stack: see *Popup menus*), which is the pattern to
  copy for anything else that has to position a child at a negative inset. A full-window layer is
  the wrong shape for it — that swallows every click meant for the app, per the overlay rule below;
  the layer has to shrink-wrap the thing it hoists.
- **Absolute overlays** (placeholders, action bars, badges) intercept clicks — every one that covers
  something clickable needs `.pointer_events(|| false)` so clicks fall through. Out of flow is not
  out of the hit test: Floem walks a view's children back-to-front looking for a pointer target and
  stops at the first one whose bounds contain the point, whether or not that child handles the event
  (`floem-0.2.0/src/context.rs`, `unconditional_view_event`: the loop is
  `for child in children.into_iter().rev()` and ends `if event.is_pointer() { break }`), and an
  overlay is typically the last child. A view that has opted out is `continue`d past instead —
  `EventCx::should_send` returns false on the flag and the walk reaches the sibling underneath
  (`Decorators::pointer_events`, `floem-0.2.0/src/views/decorator.rs:175`); Floem's own inspector
  marks its overlays the same way. It bites even when the overlay renders **nothing**, because an
  empty box still has bounds: the schema tree's size badge, once it became a panel-wide absolute
  box, swallowed every click on a table row's chevron and name with the size column switched *off*
  as well — which is what made the breakage look unrelated to the feature that introduced it.
  **The opt-out is not inheritable, so it is no help to an overlay that has its own hit targets.**
  `should_send` is asked about the *child*, and answering no `continue`s past that whole subtree —
  the flagged view's descendants are never offered the event, however much they want it. So an
  overlay that both covers the window and contains something clickable cannot be one view: it has
  to be spread as loose siblings, each small enough to be skipped on a miss. The window's eight
  resize zones (`ui::window_chrome::resize_zones`) are exactly that shape, and they were a
  full-window wrapper first — one holding no handler at all, on the theory that a view returning
  `Continue` passes the press on. It does not; the walk had already ended at it, and **nothing in
  the app was clickable**.
- **And nothing bounds them.** An absolute child is out of flow, so text in one that is longer than
  the box lays out at its natural width and **paints across the border** into whatever sits beside
  it — not clipped, not ellipsized. `edit_field`'s placeholder did this for every field in the app
  whose placeholder outgrew its width, and it is invisible until someone opens that exact modal (it
  was found by eye on the view editor's SQLite **Column names** row). The fix is an inset on *both*
  sides — `inset_left` **and** `inset_right`, which is what gives the overlay a definite width — plus
  `width_full().min_width(0.0).text_ellipsis()` on the text inside it: `width_full` because a label
  otherwise sizes to its content and overflows the box meant to bound it, `min_width(0)` because
  without it the label refuses to shrink below that content width. `placeholder_right_inset` is where
  `edit_field` decides how much room the in-flow trailing action needs.
- **A row's right edge is not the panel's, and an absolute inset is measured from the *border* box.**
  Two facts that only bite together. Rows inside a horizontal scroll stretch to the **widest** row,
  not to the viewport — the SCHEMA tree is deliberately not `width_full` so it can scroll — so a
  `flex_grow` spacer right-aligns a child to whatever the longest row happens to be. The schema
  tree's size column was pushed out that way and disappeared off the right of the viewport the
  moment any table was expanded, its indented column rows having widened every row in the tree.
  The fix is to leave the flow: `absolute().inset_left(0).width(<the panel width>)` with
  `justify_end()`, and `inset_left` rather than `inset_right` because the right edge is the moving
  one. That works at every indent level because taffy resolves an absolute child's inset from the
  parent's **border** box and never adds its padding (taffy 0.4.4, `compute/flexbox.rs`,
  `perform_absolute_layout_on_absolute_children`: `offset_main = start + border.main_start`), so one
  inset lands identically on rows carrying different per-level `padding_left` — no depth arithmetic.
  What you give up is the flow's collision handling: the in-flow sibling now runs *under* the
  overlay instead of pushing it along — in paint order and in the hit test both, so the overlay
  also needs the `.pointer_events(|| false)` the absolute-overlays bullet above is about.
- **Deferred layout**: `exec_after(Duration::ZERO, …)` runs after layout settles — so
  `scroll_to(bottom)` clamps against new content height, not stale.
- **`.get()` clones the whole value — use `.with()` to read part of a collection.** `SignalGet::get`
  is documented as *"try to **clone** and return the current value"*, so `widths.get().get(ci)`
  allocates and frees the entire `Vec` to read one slot, and `expanded.get().contains(&k)` clones the
  whole `HashSet` to answer one lookup. Harmless once; ruinous in a **per-item** closure, where it
  turns an O(n) update into O(n²) — the ERD's position map cost 8.4 ms per pointer move at 500 cards
  before the fix. `.with(|v| …)` borrows and tracks identically, so it is a drop-in. This was the
  review's most-repeated mechanical defect: reach for `with` on any collection, and keep `get` for
  small `Copy` values (`bool`, `f64`, `Option<usize>`).
- **`RwSignal::set` never dedups** — setting a signal to its current value still notifies, re-running
  dependent `dyn_container`s (which dispose + rebuild their child scope + owned signals). Guard panel
  reveals: `if !matches!(right_panel.get_untracked(), Ai) { set(Ai) }` — a redundant `set(Ai)` while
  the AI panel is open disposes its `elapsed_ms` mid-update and the rebuilt footer panics on the
  freed signal.
  **`update` doesn't dedup either, and can't** — `floem_reactive`'s `update_value` calls
  `run_effects()` with no equality check, so `sig.update(|c| c.clear())` on an *already empty*
  collection is not the no-op it reads as. `discard_edits` cleared all three staging collections
  unconditionally, and the grid body's `dyn_container` key holds `new_rows.len()`: discarding one
  cell edit tore the body down, recomputed the sort order over every row and built a new one to
  arrive at the same `0` — and moved `focus_id` out from under the keyboard hand-back that the same
  discard had already put in flight. `grid::clear_if_any` is the guard (`Clearable`, over `Option`
  as well as the collections, so "the editor is already closed" is the same case), and
  `grid::clear_tests` pins the floem fact itself by counting effect runs.
  **A `dyn_container` key re-runs on notification, not on change**, which is the same fact where it
  costs most. The view has no equality check of its own — `create_updater` calls `on_change` on every
  re-run and `swap_val` then disposes the child scope and rebuilds it unconditionally — so a key
  closure reading the target signal *directly* rebuilds the whole modal on any write to it, including
  one that leaves `is_some()` exactly where it was. Both DDL editors that fetch something after
  opening were spelled that way, and a fetch patching `current` is such a write: landing mid-keystroke
  on a slow link it took the caret with the disposed scope (floem clears `app_state.focus` when a
  view is removed), the following characters went nowhere, and `FocusRing`'s remembered cursor reset
  with the ring. `widgets::overlay_open_key(session, open, over)` is that key as a **memo** — *am I
  open, and is something stacked over me?* — taken by the routine and view editors, with
  `an_overlay_key_ignores_a_patch_to_what_it_is_keyed_on` counting rebuilds against both spellings.
  **The `session` term is what makes dedupping the two bools safe:** presence alone answers
  `(true, false)` on both sides of *reopening the editor on a different object*, so the form would
  keep the one that just closed; `DdlUi::session` is bumped by each editor's `open` and written
  nowhere else, so it says "these contents were replaced wholesale" exactly when a fetch's patch does
  not. **The trigger editor's key is deliberately left un-memoised**, and that asymmetry is the part
  a tidy-up would break: its `form` is built once per `(selected, rev)` and its Body field seeds at
  build, so there the rebuild is the only thing that delivers MySQL's escape-corrected trigger body —
  dedup that key and a trigger silently goes on editing `information_schema`'s resolved copy. Doing
  it properly needs a body signal per row, the way the routine editor got one; there is a note at the
  key saying so. The view editor took the memo safely for the opposite reason: `fetch_algorithm`
  patches a term in the diff and not a control, so its rebuild delivered nothing to the screen.
- **An effect that writes the signals it reads must read them untracked — and an outside write to
  them is then invisible, so it needs a generation counter.** The schema tree's size-column effect
  scans every `ConnNode::stats` slot for `Idle` and writes `Loading` into each one it fetches;
  tracking those reads would make the effect its own dependency, re-entering it mid-loop and
  double-fetching every database it had not yet reached. `get_untracked` there is load-bearing, not
  an optimisation. The half that is easy to miss is the other side: a refresh resetting those same
  slots to `Idle` now changes nothing the effect watches, so the sizes went blank and only returned
  when an unrelated dependency happened to re-run it. The answer is a bare counter beside the state
  — `main.rs`'s `stats_gen: RwSignal<u64>`, bumped by `start_fetch` immediately after the reset and
  `track()`ed by the effect. **Every consumer of those slots has to take that dependency, not just
  the one the counter was added for**: the results toolbar's asker read nothing tracked, so it ran
  once per grid build while the memo beside it stayed live on the slot — the half that can only *lose*
  the figure. It reaches the UI as `SchemaUi::stats_gen` and travels to the grid on `GridCtx`. Don't
  lean on a state signal that "obviously" already changed: the
  connection-wide refresh does `set` `db_nodes`, which the effect *does* track, but it does so
  before `start_fetch` resets the slots, so that run still saw them `Loaded` and found nothing to do.
- **Don't read a locally-scoped signal inside a `dyn_container` child keyed on a *parent/shared*
  signal.** The child rebuilds when the shared signal changes — and if it changes *while the
  enclosing view is disposing* (e.g. `active_db` updates as the query pane is replaced on opening a
  table), the rebuild reads the freed local signal → panic. Fix: read the local signal in a stable
  outer scope and let the child inherit (put the hover `color()` on the parent `h_stack`, not the
  `active_db`-keyed child). Reading *global* signals (theme, `connections`/`active_conn`) there is
  fine — they never dispose.
- **No `text-align` in Floem 0.2.** `text_input` paints at a fixed left origin, clips-to-cursor on
  overflow; `Style` only has `text_overflow`. To right-align an inline editor (numeric grid cells),
  pad left by the free space — measure with a throwaway `TextLayout` at `FONT_BODY` (same global
  `FontSystem` → pixel-exact), recomputed reactively on the buffer (`grid::measure_text_px`).
  **"Free space" is the cell's real content box** (`grid::numeric_edit_pad_left`, tested): the column
  width less its padding *and* its 1px right divider, which is a border and so comes out of the
  content too. Floem sizes the input's inner text node to `content − padding_left` and clips as soon
  as the text is one pixel wider — **on a glyph boundary**, so a 5px error is a whole missing
  character: a padding computed against `col_w − 20` showed one digit of a 2-digit id and *nothing*
  for a 1-digit one. Leave a couple of px of slack; the node width goes through an f32 percentage
  resolution, and landing a hair under the text width costs a digit, not a hair.
- **SQL editor padding is a no-op; inset via a wrapper.** The editor is a scroll view — its own
  `padding_*` is ignored. Wrap it in a container carrying the border + padding (`editor_box`); the
  editor fills it flush. Top padding shifts the content origin, so `points_of_offset`-anchored
  overlays (completion popup, statement highlight, squiggles, Ctrl+K, run menu) each add back
  `EDITOR_PAD_TOP` to their `y`. The built-in scrollbars float at the *content* edge (can only inset
  inward), so they're **replaced with custom overlay scrollbars** (`v_scrollbar`/`h_scrollbar` in
  `editor_area`): built-in bars hidden (zero-`Thickness` + transparent `Handle`), two `empty()`
  thumbs pinned to the border (`inset_right/bottom(3)`) with `autohide_state()`. Geometry from
  `ed.viewport` (offset `x0`/`y0` + visible `width()`/`height()`) vs. content (`ed.max_line_width()`,
  `(ed.last_line()+1) * ed.line_height(0)`; `ScrollBeyondLastLine` = false), in `v_geo`/`h_geo`
  shared by the style closure and drag handler. Thumb `.style()` reads `viewport.get()` **and**
  `query.get()` (content size isn't a signal). **Draggable**: `PointerDown` records grab offset +
  `id.request_active()` (pointer capture); each `PointerMove` sets `ed.scroll_to.set(Some(Vec2))`
  (it's `Option<Vec2>`, not `Point`). Thumbs use `scrollbar_hover()` + `CursorStyle::Default`.
- **Shift+wheel → horizontal scroll in the editor.** The editor owns its scroll internally, so
  `shift_hscroll` can't reach it. Register a `PointerWheel` listener on the internal scroll view —
  reached via `ed.editor_view_id.get_untracked().and_then(|c| c.parent())` (the content view's parent
  *is* the scroll) + `ViewId::add_event_listener`. Floem's `Scroll` runs registered listeners in
  `event_after_children` before its default scrolling, so returning `Stop` for shift+wheel suppresses
  vertical scroll; push the horizontal delta through `ed.scroll_delta` (Windows delivers shift+wheel
  as vertical `delta.y` → map to x). Event flow: a child's `event_after_children` runs *between* the
  parent's before/after, and a child that consumes a pointer event stops propagation — so a
  `PointerWheel` `on_event` on an ancestor never sees a wheel the inner scroll consumed. Target the
  scroll's own `ViewId`.
- **A programmatic `cursor` change shows a phantom caret on an *unfocused* editor.** `edit_field`'s
  signal→doc reconcile sets `cursor` to preserve caret position, but floem's internal "reset cursor
  blinking" effect tracks `ed.cursor` → `cursor_info.reset()` → shows + blinks the caret even with no
  focus (made Name/User/Password look focused). Fix: only write `cursor` in the reconcile when
  `focused.get_untracked()`. The stale offset is safe — floem clamps `offset.min(text.len())` and a
  real click resets it. (Don't hide the caret *after* the write — floem's reset effect runs after
  yours and re-shows it.)
- **A perpetual self-rescheduling `exec_after` tick must read signals with `try_get_untracked`.** The
  terminal cursor-blink tick reschedules forever; at shutdown the scope disposes its signals and the
  last timer panics on `get_untracked`. Guard every read with `try_get_untracked` and stop
  rescheduling once any returns `None`.
- **`.clip()` makes a flex item shrink-to-content — it won't stretch to its parent**, so a
  `flex_grow` spacer inside it collapses (right-aligned children stop reaching the edge). Put
  `.clip()` on a container with a *definite* size (the fixed-width `v_stack`), not the flex row you
  depend on for stretch. (Bit the find/replace bar's `All` alignment.)
- **`s.hide()`/`s.flex()` (display none/flex) beat height/scale for a reactive show-hide** — adds/
  removes the element from layout cleanly (no clip/overflow/leftover space). Prefer it to animating
  height when you don't need the animation.
- **In-flow reveal animations are janky; only `.absolute()` transforms animate smoothly.** A
  `.transition(Height, …)` on an in-flow element reflows its container every frame and Floem only
  steps transitions on redraw ticks → ~5fps. Smooth animations here (Ctrl+K expand) animate an
  `.absolute()` overlay's inset/size so nothing reflows. Either animate an absolute overlay or toggle
  `display`.
- **`Style::rotate` is RADIANS** (kurbo `Affine::rotate`, centre-pivoted) — pass `FRAC_PI_2` for 90°,
  not `90.0` (~14 turns → glyph vanishes). `scale` is a `Pct`. Both transition via
  `.transition(Rotation/ScaleX, …)`. Even correct, a transform-transition on a small `svg` proved
  unreliable — an icon swap is the safe chevron-flip fallback.
- **`edit_field` lets you control Escape/blur; `text_input` eats Escape.** `text_input` handles
  Escape internally (`clear_focus`, `event_before_children`, returns `Stop`), so your
  `on_event(KeyDown)` never sees it. `edit_field` routes Escape to `on_escape` and focus-loss to
  `on_blur` (guarded to skip the mount run) — use it for discard-on-Escape / commit-on-blur (inline
  rename, find/replace).
- **A key event goes to the focused view and, if consumed, to *nobody* else.** The dispatch is
  `directed` — no bubbling to ancestors, and with focus on nothing only the root view's own
  listeners run. Floem's editor consumes every `KeyDown`, so a focused `edit_field` used to swallow
  Escape and leave the enclosing modal unclosable from the keyboard. The fix is a two-step the user
  can see: Escape in a field with no `on_escape` of its own blurs it and hands focus back to the
  innermost mounted `widgets::focus_root`, and the *next* Escape reaches that overlay's handler.
  So an overlay that wants keys goes through `focus_root` (which is also what registers it) —
  `.keyboard_navigable().request_focus(|| {})` on its own leaves it deaf as soon as a field is
  focused. A field is *not* a focus root (the grid's inline cell editor deliberately isn't one).
  **The same two-step covers an overlay over an overlay, and it is the `on_cleanup` half that
  makes it work.** Floem clears `app_state.focus` when a focused view is removed and does it
  *silently* — `if self.focus == Some(id) { self.focus = None }`, no `focus_changed`, so no
  `FocusGained` lands anywhere — so a popup menu opened over a modal took the keyboard and gave it
  back to nobody: Escape closed the menu and then the modal answered nothing, while its close
  button and Cancel still worked. `focus_root`'s cleanup therefore hands focus to the new innermost
  root. That the first Escape closes the *menu* and the second the modal is the intended
  behaviour — one keypress dismissing two layers is the bug, not the fix. Don't chain your own
  `.on_cleanup` onto a `focus_root`: floem keeps one cleanup slot per view, so a second silently
  replaces both the unregister and the hand-back.
- **A view that takes focus *on mount* must first check that no overlay owns the keyboard**
  (`widgets::innermost_focus_root().is_some()` → don't). The query pane focuses its editor when it
  is built, which is right for every route it was written for — each is a tab the user just asked
  to look at — but the pane is also rebuilt whenever the *active tab* changes, and that can happen
  behind an open modal: deleting a connection from Manage Connections takes its tabs with it, so
  the editor stole the keyboard out from under the modal, which then had to be clicked again
  before it answered Escape. The registry is the honest test — a list of the modals that are open
  would rot on the next one added.
- **The results toolbar is the one `FocusRing` outside an overlay.** Every other ring belongs to a
  modal, and a ring *wraps* precisely so a modal's Tab order is a trap — which is why the workspace
  has none. Scoped to a single strip that property is the right one: the walk stays in the toolbar
  instead of falling into floem's whole-window traversal, and **Escape** is the deliberate way out
  rather than the last Tab. **F6** enters it from the grid body (`step_from` with the body's own
  non-member id, so it enters at the first control or resumes where the strip was left, and arms
  `keyboard_nav` on the way); ←/→ walk it, Tab does too, Enter/Space activates. `widgets::in_strip_button`
  is `in_ring_button` plus those two keys, and its `leave` must **defer** its focus request —
  `in_focus_ring`'s own Escape arm runs too (floem folds every KeyDown listener) and queues a
  `ClearFocus` in the same pass, so only a request landing in a later tick wins. Two smaller rules
  ride along: a disabled control is not a ring member, so the block holding − and clone tracks the
  row selection as well as insertability or Tab would walk onto controls that had since gone live;
  and closing a menu raised from the strip has to hand focus **back to the icon**
  (`widgets::set_menu_return`), because the panel is a `focus_root` with no other root above it out
  here and its teardown would otherwise drop focus entirely — which left F6, a listener on the grid
  body, with nothing to fire on.
- **Tab navigation is ours, not floem's, and a modal's Tab order is a `widgets::FocusRing`.**
  Floem has `view_tab_navigation`, and it is unusable here on three counts: it is `pub(crate)`; it
  walks the *whole window tree*, so Tab would leave the modal for the workspace behind it; and it
  only runs when **nothing consumed the key**, while floem's editor registers KeyDown with
  `on_event_stop` and so swallows every key — a focused field being exactly the case Tab must work
  from. So a control joins the ring: `widgets::in_focus_ring` for anything but a text field (it adds
  `keyboard_navigable`, Tab/Shift+Tab, and the Escape blur), and `FieldCfg::focus` for a field,
  which carries the ring *inside* the editor's own key handler because nothing bolted on from
  outside can see a key there. It registers the **inner** editor view — the id that actually takes
  focus — and withdraws it on unmount. Order is an explicit `tabindex`, spaced, **not** registration
  order: a section built later (the SSH block, once its toggle is on) would otherwise register after
  the fields below it on screen. A dropdown joins through `settings::in_ring_dropdown`, which is the
  whole four-work-around apparatus below in one place; `focusable_dropdown` (settings' `Copy`
  picker) and `table_designer::focusable_owned_dropdown` (the designer/editors', since a table name
  isn't `Copy`) are both thin wrappers over it, and neither has an un-focusable sibling left —
  a second one would only be a way to leave a control out of the ring by accident.
  **The modal's root is what *enters* the ring**, via `widgets::focus_root_with_ring`. A key goes
  to the focused view and, unhandled, only to the window root — and a modal opens with focus on its
  own `focus_root`, which is also where Escape hands it back. Without a Tab handler there the ring
  could only be joined by clicking a control first, so Tab did nothing at all on a freshly-opened
  modal. The root isn't a ring member, so `step_from` falls back to the ring's **remembered cursor**
  (`FocusRing::remember`, set by every Escape blur and every step) and, on a ring that has been
  nowhere yet, starts at the first control (the last, for Shift+Tab). The memory is not a nicety: a
  `tab_indents` field can only be left by Escape, so re-entering at position 0 made every control
  after it unreachable by forward Tab.
  **The window root is the backstop** (`lib.rs`'s root KeyDown, beside the Escape branch). A plain
  Tab reaches it only when nothing in the overlay consumed the key — focus is on a dropdown's popup
  list, or on *nothing*, which is what floem leaves behind whenever a focused view is removed or an
  unfocusable row is clicked — and floem's own fallback then walks the whole window tree. So the
  root steps `widgets::innermost_ring_root()` instead, which is why `FOCUS_ROOTS` carries
  `(ViewId, Option<FocusRing>)` rather than a bare id.
  **Every cleanup that can run while focused hands the keyboard back** — `focus_root`,
  `in_focus_ring` and `edit_field` all do, because floem clears `app_state.focus` *silently* on
  removal (no `focus_changed`, so no `FocusGained` lands anywhere) and the modal around the
  departing control is then deaf to Escape and Tab alike. One click on the designer's list `+` was
  enough. "Was I focused?" is mirrored into an `Rc<Cell<bool>>`, never read back from a signal that
  may already be disposed. Note floem keeps **one** cleanup slot per view, so a control needing its
  own teardown passes it to `widgets::in_focus_ring_with` rather than chaining a second
  `.on_cleanup`, which would silently replace the unregister *and* the hand-back.
  **And there is somewhere to hand it to outside a modal**, which there was not:
  `innermost_focus_root()` is `None` in the workspace, so a ring control that removed itself while
  focused — the results toolbar's ✓/✗ pressed from the keyboard does exactly that, since the commit
  block's `dyn_container` key is what they change — left *nothing* focused, and from there the grid
  answered no key at all, F6 included, until the user clicked a cell. `hand_keyboard_back` now falls
  back to `widgets::set_keyboard_home`: an action the workspace registers, rather than an ancestor.
  It has to be a place and not a root — a modal's root carries the key handlers for its own contents,
  while the workspace's carries almost none (the arrows, `Del`, `Ctrl+Enter`, `Ctrl+F` and `F6` are
  all listeners on the results grid's body) — and a closure rather than a `ViewId`, because a grid
  rebuilds per result and a stored id names a view that has gone. `grid_view` registers
  `refocus_grid` where it sets `focus_id`; `refocus_grid` reads that signal with `try_get_untracked`,
  since the home outlives the grid that registered it.
  **And it reads it *inside* its deferred tick**, which is the other half of the same rule and was
  missing for a release: the read was hoisted out and the id captured, so the hand-back landed one
  tick later on whatever had been there when it was *scheduled*. That is exactly the window the ✗
  opens — `discard_edits` notifies `new_rows`, the body's `dyn_container` key, so the body the id
  named was already gone — and floem's focus request has no existence check
  (`UpdateMessage::Focus` assigns `app_state.focus` whether or not the id resolves), so the keyboard
  was parked on a removed view and **every** key was dropped until a click. The ✓ escaped it only
  because `commit_busy` isn't in the body's key. Defer the *resolution*, not just the request —
  `grid_toolbar`'s `focus_icon` says the same thing and resolves by tabindex.
  This is the durable form of what
  `set_menu_return` does for one case: fixing the sites one at a time is what produced the tree's
  cursor regression below.
- **A `.hide()`n control is still in the Tab order** — `hide()` is `display: none`, so the view is
  still in the tree and still registered in the ring, and Tab moves focus onto something nobody can
  see. Every engine-conditional block that was built-and-hidden is therefore now **built
  conditionally**: import's CSV settings, the designer's MySQL-only engine/collation and `ON UPDATE`
  and PostgreSQL-only index method/predicate, the view editor's MySQL options, PG recreate toggle
  and SQLite-only column list, the trigger form's `Fires`/`When` and SQLite-only `Of columns`.
  Nothing is lost by rebuilding — each of those binds
  straight to a draft or a persisted signal — and a control an engine can't express shouldn't be
  reachable at all, which is the same call `trigger_editor`'s per-engine form already made.
  **The else-arm is still `display:none`, via `widgets::nothing()`** — taffy skips a `display:none`
  child when it distributes `gap` but counts a zero-sized one, so a bare `empty()` arm leaves a
  whole `FORM_GAP` of dead space where the block would have been. The rule is about *controls*: an
  arm with nothing inside it has nothing to be Tab-reachable. Where the conditional is a
  `dyn_container`, the hide goes on the **container** — that is the flex child, not its inner view.
- **Buttons are in the ring too, and Space or Enter presses them — but there is no default Enter.**
  Every button a modal has goes through `widgets::in_ring_button` (which the builders —
  `action_button`/`action_button_icon`/`action_face`/`control_button`/`control_button_enabled`/
  `row_button`/`dialog_button` and `modal_title`'s ✕ — call for you; the ring parameter is
  *required*, so a modal button that isn't reachable won't compile). Enter in a *field* fires
  nothing: the DDL preview's Apply is an
  irreversible `ALTER`, and a key meaning "newline" in one control and "apply the plan" in another
  is the shape of defect the ring's own review was full of. **A disabled button is not a stop** —
  it keeps its place on screen (which action is affirmative shouldn't move as a form becomes valid)
  but the keyboard walks past it, since its click handler is inert anyway.
  That is a **build-time** decision — `enabled` is a plain `bool`, not a signal — so a control whose
  availability changes while the modal is open has two spellings and they are not interchangeable.
  Either the block holding it is rebuilt when availability changes (the results strip's − and clone,
  which track the row selection as well as insertability, or Tab would walk onto controls that had
  since gone live), or the control is registered unconditionally and the action guards itself. The
  Live Monitor's Clear and Export take the second: they *dim* on an empty log rather than leaving
  the ring, because rebuilding would reflow the sub-header the moment the first change lands, and
  Enter on a dimmed one does nothing because each closure re-checks the log. Passing `has_log()`
  into `enabled` is the tidy-looking edit that breaks this — it is read once, at build.
  **The ring member is a wrapper `in_ring_button` builds, never the caller's own view**, and that
  is a correctness rule rather than a layout preference. Two things resolve by exact `ViewId` with
  no descendant propagation, and they were resolving to *different* ids depending on each call
  site's decorator order: floem fires `EventListener::Click` on the **focused** view for any
  physical Enter/Space and then folds every registered `KeyDown` listener, so registering a face
  that already carried `on_click_stop` made the ring's own arm a **second** activation (one Space
  added two columns, opened two file dialogs, started two bulk imports); and `.focus(…)` resolves
  by exact id too, so a face decorated with `.tooltip()` — a fresh `ViewId` — put an id in the ring
  that carried no outline. A caller therefore styles only the *face* (padding, hover, the click
  listener, its tooltip) and never applies `button_focus_ring` itself;
  `widgets::a_ring_button_registers_a_wrapper_not_the_face_it_was_given` pins which of the two views
  is the one registered.
  Order is `NAV_TAB` → `LIST_TAB` → the form (10, 20, … within a section, by 100 between them, up
  to `FIXED_TAB_END`) → `VALUE_TAB` + `i * ROW_TAB_STRIDE` for a growing list → `ACTION_TAB` for
  the footer → `TITLE_CLOSE_TAB` for the title bar's ✕ (last, since
  it is the same action the footer's Cancel already offers), and that chain is asserted at
  **compile time** in `widgets.rs` (`const _: () = { … }`). It has already caught one regression:
  adding `ROW_TAB_STRIDE` cut the footer's headroom tenfold the day it landed.
  The compile-time chain relates *constants* only and cannot see a number a control actually
  claims — the import modal's mapping rows claimed `100 + i * 10`, a growing block based in the
  fixed range, with the build green. **A per-registration check cannot close that gap, and trying
  it caused a crash**: `FocusRing::register` is handed one index at a time, so a legitimate fixed
  control at 200 (Settings' row-limit dropdown) and the first row of a misplaced block at 200 are
  indistinguishable there — a band `debug_assert` asserting fixed controls end at 110, a number
  taken from a stale comment while Settings really reaches 310, panicked the app on correct code.
  `register` therefore asserts only the **ceiling** (nothing past the ✕). What covers the real
  rule is that every growing block reads `VALUE_TAB + i * ROW_TAB_STRIDE` — four sites, greppable
  — and `ring_tests::every_band_the_app_uses_registers_cleanly`, whose list is the app's real
  indices rather than an idealised one. Check a guard against reality before checking it against
  intent.
- **A modal's click-to-dismiss goes on `widgets::dismiss_layer`, never on the `focus_root`.** Floem
  fires `Click` on the focused view for Enter and Space, and a modal opens with focus on its own
  root — so `.on_click_stop(close)` there meant **Space closed the modal**. On the Live Monitor
  that also stopped the poll and emptied the change log (deletes included, which is the one record
  of a row that is gone); on the confirm dialog it answered `false` to a question nobody had read.
  The layer is an absolutely-positioned sibling built *before* the panel, so the panel stays on top
  of it. The transaction prompt deliberately has none: clicking away from a question about
  uncommitted writes is not an answer.
- **An overlay's `inset(0.0)` style and its content must read the *same* predicate** — one closure,
  used twice, never two spellings of "is this showing". A dropdown whose content is conditional on
  more than its open flag (`open && !db_nodes.is_empty()`, the rule that keeps an empty menu off
  the screen) but whose style still tests `open` alone stretches a transparent, handler-less
  container over the whole window with **no panel and no `dismiss_layer` on it**. It swallows every
  click in the app — including the one on the trigger that would close it — and the Escape handler
  it would need lives inside the content that never mounted. The window repaints perfectly and
  answers nothing, so it reads as a hang and ends in a killed process; the active-database menu
  shipped exactly that, reachable from any connection with no databases. The trigger **guards its
  own launch** in the same step (the `widgets::accept_launch` rule, applied to a menu), and the
  overlay clears a flag whose panel can't render, so a later load can't pop a menu nobody asked for.
- **The QUERY toolbar's database selector names a database only while the connection has loaded
  it** (`core::schema::shown_database`), and reads "No database" otherwise. A tab's `database` is
  *saved state* that outlives the connection being reachable — it has to, or a server coming back
  would leave every tab pointing somewhere new — but a connection that loaded nothing shows an
  empty tree and a "Disconnected" header, and a toolbar still naming a database is the one surface
  claiming otherwise, when that name can't be listed, selected or read. So the rule decides the
  **label**, and the binding is left alone for a recovered connection to restore from. Membership
  rather than "the list is empty", so a database dropped server-side stops being named the moment a
  reload no longer carries it — the same test `set_active_db` already applies before selecting one.
  A **button's** focus signal is an outline, painted in `.focus`, *not* `.focus_visible` — floem
  gates `FocusVisible` on `app_state.keyboard_navigation`, which only its own `view_tab_navigation`
  ever sets, so a `focus_visible` rule on a ring member usually never fires at all. A **group**
  (below) deliberately shows nothing.
  **`widgets::keyboard_nav` is the app's own `:focus-visible`**, and the reason the ring can be
  `accent` rather than something apologetic. A focus outline is information the *keyboard* needs;
  under the pointer it marks what you just clicked, so a ring bright enough to be useful under Tab
  is noise under the mouse — which is why it used to be `field_border_active`, `#303453` against
  the dark panel, a shade off the surface it sat on. The flag is **set in `FocusRing::step_from`**
  and **cleared on the root's `PointerDown`**, and both halves are the way they are for a reason:
  every keyboard-driven focus change in the app is a Tab through the ring, so `step_from` *is* the
  definition (no key allowlist to keep in step, and typing in a field can't arm it), while a key
  listener on the window root — the obvious spelling — would have missed the only case that
  matters, since floem bubbles a key to the root **only if nothing consumed it** and Tab is exactly
  what the ring consumes. `FocusRing::focus_at` and `hand_keyboard_back` deliberately leave it
  alone: both move focus on behalf of something the user may have reached either way (a dropdown
  handing the keyboard back, a field unmounting under them), so the last real gesture stands.
  It rides in a detached-scope `thread_local` signal, the shape `window_size` and `pointer_released`
  already use. Buttons, the Settings toggle switch and the colour swatches all read it.
  **A view that swallows a pointer-down inherits the clearing**, and forgetting that is a live bug
  rather than a stray outline. A menu trigger must stop the press so the root's "close on down"
  doesn't fire for the click it is about to act on — and stopping it takes the root's `keyboard_nav`
  clear with it. Left set, `set_menu_return` is armed for a menu opened **by mouse** (the opener asks
  the flag in the `Click` that follows), so closing that menu drags the keyboard back to the trigger
  and pulls the arrow keys off whatever had them — the grid's cell navigation, in the case that
  motivated the conditional slot. `widgets::menu_trigger_press` is that handler, and every trigger
  installs it in place of a bare `|_| {}`: the grid's Copy / Save / AI icons, the status-bar
  segments, the schema panel's eye and gear, the Server Activity clock, the connection switcher, the
  QUERY toolbar's database selector, and `suggest_chevron`. The *panels* that swallow a
  press keep the bare closure, and the distinction is the point: they do it so a click inside isn't
  read as a click away, and where focus goes when one closes was settled by `set_menu_return` when
  it opened, not by the press that dismisses it.
  **Gate the ring on it and nothing else — least of all the answers to floem's own defaults.**
  `settings::themed_toggle` briefly put its whole focus block behind the flag's early return, and
  the flag being false is exactly the mouse path, so a click on the switch got floem's
  `ToggleButtonClass` styling back: a 1px `#8c8c8c` border (grey around the dark off-track,
  invisible on the lit on-track), the faint magenta ring users reported, and a near-white
  `#eae6ec` track when focused *and* hovered. All three come from `floem-0.2.0`'s
  `theme::default_theme` — the magenta is `border_color(#724a8c)` applied on plain **`.focus`**, not
  `.focus_visible`, which is why the pointer saw it and the keyboard never did. The border is
  answered with `.border(0.0)` rather than a transparent colour: the handle is positioned from
  `layout.size`, so the width costs no geometry, and a border that cannot paint is a border floem's
  `.focus` rule cannot colour. `button_focus_ring`'s identical early return is safe only because it
  decorates a bare wrapper container, and floem's border/focus defaults reach widgets through
  **classes** (`ToggleButtonClass`, `ButtonClass`, `TextInputClass`), never plain containers — so a
  view sitting on a classed widget has to answer those defaults on every path, not inside a branch.
  **And a ring must set `focus_visible` to the same outline as `focus`, or answering those defaults
  erases it.** Floem applies the `Focus` map first and the `FocusVisible` map after
  (`style.rs`'s `apply_interact_state`), gating the second on `app_state.keyboard_navigation` — which
  latches **globally** the first time floem's own Tab traversal runs anywhere in the window and is
  reset by nothing but a pointer press onto a navigable view. So a control that suppresses
  `focus_visible` (as every one here must, to kill floem's 3px magenta) and paints its ring only under
  `focus` shows *no focus indication at all* from the first Tab onward. `button_focus_ring` sets the
  pair; `settings::toggle_focus_ring` — extracted for exactly this — now does too, and
  `widgets::ring_tests` asserts over the composed `Style` that a ring's `FocusVisible` outline is
  never narrower than its `Focus` one, for both builders.
  **The outline follows the face's corner radius, and `in_ring_button` has to be told it.** Floem
  strokes an outline at *the painting view's* `border_radius` (`view::paint_outline`), and the ring
  member is a wrapper around the face — so with no radius on the wrapper every rounded button in
  the app wore a **square** ring. `widgets::ACTION_RADIUS`/`CONTROL_RADIUS` are read by both the
  face's own style and the wrapper, so the two cannot drift; icon buttons pass `0.0`, their faces
  being genuinely square.
- **A group of like things is *one* Tab stop, and arrows move within it.** Manage Connections'
  colour swatches (`connection_form::color_picker`) and connection list, the designer's section
  strip, and the designer/trigger item list (`table_designer::list_pane`) each take a single ring
  slot: Tab reaches the group, Left/Right or Up/Down move inside it, Tab leaves. `widgets::nav_group`
  is the shared one (the swatches and `list_pane` predate it and carry their own scrolling and
  chrome). Eight swatches or twenty columns as individual stops would
  make crossing a pane the user is only passing through cost twenty keypresses.
  **A `nav_group` paints no focus indication**, unlike a button: it wraps a whole bar or pane, so
  an outline around it reads as a stray border — and it doesn't need one, because what the arrows
  move is already highlighted, so the first press announces the focus by moving the thing you were
  looking at. Floem's own 3px magenta ring is still suppressed (`focus_visible → outline(0)`),
  since `keyboard_navigation` latches globally once floem's traversal has run anywhere.
  `list_pane` makes the opposite call and recolours the border it already has — it has one to
  recolour, which costs no layout; that is the test, not a house style. Two details each
  had a reason: the **ring wraps and these clamp** — wrapping is what stops Tab escaping the modal,
  while a selection that jumps from the last column to the first is only a surprise (the swatches
  *do* wrap, being a short fixed ring where the ends are visibly adjacent) — and the group's focus
  indication must cost **no layout**, so the list recolours the border it already has and a swatch
  takes an `outline` (painted outside the box) rather than a border that would nudge the row along.
  The swatch cursor is a signal of its own, cleared on `FocusLost`, so the halo never outlives the
  focus it stands for — and it is gated on `keyboard_nav` like a button's ring, because it says
  which swatch the *arrows* will land on, while which one is **chosen** is already said by the
  white border it wears.
- **In a field holding *code*, Tab is typing.** `FieldCfg::tab_indents` suppresses only the ring's
  step-away, so Tab still *arrives* at the field and floem's own `InsertTab` then runs; Escape is
  the way out, blurring to the `focus_root` whose Tab re-enters the ring **after this field** (the
  blur calls `FocusRing::remember`, which is what makes a mid-ring placement legal). Set on the
  trigger body,
  the PG function body and the view editor's `SELECT`. Deliberately not on prose (the AI settings'
  custom instructions) or on the DDL preview's read-only script box, where there is no indent to
  type and Tab moving on is simply better.
- **A popup that takes the keyboard can only be closed from the *window root*.** The corollary of
  the directed dispatch above, and it is not obvious: a modal's Escape handler runs because its
  `focus_root` is the *focused view*, not because it is an ancestor — there is no bubbling in
  between. So while a dropdown's popup is up, the box that owns it, its ring, and the modal around
  it are all bypassed, and floem's list returns `Continue` for Escape, so the key reaches the root
  and finds nothing. `widgets::set_open_popup`/`dismiss_open_popup` is that slot (thread-local: only
  one popup can be up), consulted from `workspace`'s root KeyDown fallback. Escape then peels one
  layer per press — root closes the popup, `in_focus_ring` blurs the control, the modal closes.
  **The slot is tagged with a `PopupToken`, and a control may only clear its own entry.** Floem
  queues B's open during dispatch and A's close at the end of the *same* event, so opening a second
  dropdown over an open one ran `set(B)` and then A's clear — and an untagged clear emptied the slot
  under B, after which Escape did nothing at all. The build-time run of each dropdown's effect (with
  `open == false`) was a second way in, so merely constructing one wiped it. `clear_open_popup` is
  also what the dropdown's `in_focus_ring_with` teardown calls: a control disposed with its popup
  still up (click the backdrop) otherwise left a closure over a dead scope in the slot, and the next
  Escape *anywhere in the app* was swallowed by it.
- **Floem's `Dropdown` toggles on `KeyUp`, which reopens it after a keyboard accept.** Enter is
  *pressed* in the popup (accepting, which closes it) and *released* over the box focus has just
  returned to, so the release opened it again — no delay fixes that, the release is tens of ms
  later. `settings::focusable_dropdown` therefore takes the open state over: `disable_default_event`
  drops floem's KeyUp toggle (its only use), `show_list` drives the state from a signal, `on_open`
  mirrors floem's own opens/closes back into it so a pointer click can't leave the two disagreeing,
  and opening moves to KeyDown (Enter/Space/Up/Down). Also: `on_accept` is a **single slot**, so
  overriding it means repeating whatever the previous one did.
- **A list item's `selected` style is the keyboard's cursor — don't neutralise it.** The dropdown
  popup blanked floem's selected tint (the resting highlight for the value in effect is applied by
  the row builder instead), which left arrowing through an open dropdown with nothing to look at:
  you could count keypresses and press Enter, but not see what you were about to choose.
- **`text_editor_keys` inserts a typed char *unconditionally* — your handler's `CommandExecuted`
  return is ignored for plain character keys.** Floem's `editor_content` KeyDown listener discards the
  handler's result and then, if `mods` (minus SHIFT/ALTGR) is empty, calls `receive_char(c)` for any
  `Key::Character`. So the existing custom edits (soft-tab, Ctrl+/, Ctrl+D…) only work because they're
  on keys that *don't* trigger that path (Tab is `Named`; Ctrl-combos keep a modifier). To fully take
  over a **plain character** (auto-close pairs), suppress the built-in insert by setting the editor's
  own `ed.read_only` true for the rest of the dispatch and restoring it on the next tick via
  `exec_after(ZERO)` — `receive_char` early-returns on `read_only`, nothing else reads it, and the
  handler→`receive_char` step is synchronous so there's no race/flicker (see the auto-pair block in
  `editor_pane`). Named keys (Backspace) never hit `receive_char`, so returning `Yes` is enough there.
- **Only *typing* may open the suggestion popup** (`completion::popup_may_open`), and what counts as
  typing is decided in the key handler (`completion::types_a_character`) because it cannot be
  decided anywhere else. Every document change schedules a recompute a tick later, and the recompute
  sees only the document — where a typed `x` and Ctrl+X are indistinguishable. So Ctrl+X (delete
  line) and Ctrl+Z opened a list wherever the caret happened to land, and Enter opened the
  `auto_show` list on the new blank line: three basic edits that never asked for a suggestion.
  `Ctrl`/`Alt` mean command, never text — the same test the auto-pair block applies, with the same
  AltGr cost — while Space is typing however the platform reports it (`Named(Space)` or `" "`),
  because the empty-prefix list after `WHERE ` hangs off it. A list already open keeps recomputing
  on any edit, so Backspace still refines it and closes it when the prefix goes; the rule governs
  what may **start** showing one. `Completion::typed` is **a one-shot**, cleared by
  `recompute_completions` as it reads it — not every edit arrives through a key at all, and a
  context-menu paste, an IME commit or dropped text would otherwise be judged by whatever the last
  keystroke was: type `sel`, dismiss the list, right-click → Paste, and the popup opened on the
  pasted text because the flag was still standing from the `l`. `Completion::suppress` is the other
  half and a different thing: a one-shot that closes an open list after an edit the app itself made
  (`edit_untyped`, accept).
- **`Editor::points_of_offset` returns *content* coords, not viewport-relative** (`.y` is `vline_y`,
  the absolute document y; the gutter view subtracts `viewport.y0` itself). Overlays pinned in
  `editor_area` must subtract `ed.viewport.get()` `x0`/`y0` to follow scroll — see `char_box`
  (bracket matching), `underline_seg_at`, `statement_line_boxes_at`, all tested against a scrolled
  viewport. The caret-anchored popups do it one step later: `completion::set_anchor` stores the
  caret line in content coords and `completion_popup`/`signature_popup` subtract the viewport
  *inside their style closures*, because those are placed once per edit and would otherwise stay
  pinned to the scroll position the popup opened at. **`editor_area` also doesn't clip**, so an
  overlay must bound itself: a box wider than the visible code column paints straight out of the
  editor and over the panel beside it, which is what `statement_line_boxes_at` clamps against
  `vp.width()` (a zero width means "not laid out yet", so it clamps nothing rather than blanking the
  overlay). For the *text* overlays the vertical half needs no clamp — floem won't place an offset
  outside its screen lines, and `editor_points` drops what it won't place. The suggestion list is
  the exception on **both** axes, since it occupies space no line does, and it bounds itself against
  `editor_area`'s tracked size (`area_h`/`area_w`) rather than against the text. Vertically,
  `completion::popup_placement` hangs it below the caret when it fits, flips it above the caret line
  when it doesn't, and shortens it to the roomier side when neither holds; left to hang below
  unconditionally it drew itself down across the results grid. Horizontally, `completion::popup_w`
  sizes it and `completion::popup_x` slides it left to keep the right edge inside the pane — a flat
  `COMPLETION_GUTTER + caret.x` ran a completion near the right edge off the pane, where `.clip()`
  cut every row's annotations off mid-word (worst with the AI panel hidden, where the editor's right
  edge *is* the window's). Both predict rather than measure, because a style closure gets neither a
  laid-out height nor width: the height from `COMPLETION_ROW_H × rows`, pinned when flipped so an
  under-estimate costs a scrolled pixel rather than an overlap; the width from
  `completion::natural_width`, which sums the row chrome (the `COMPLETION_*_W` consts, which
  `completion_popup` builds its rows from so the two can't drift) with `widgets::measure_text_px_at`
  over the row text. That measurement runs in `completion::set_items` — once per recompute, not per
  style pass — and its result lives on `Completion::width`. It replaced a flat `min_width(320)`,
  which left a list of one-letter column names three-quarters empty; a row too wide for the box now
  ellipsizes its two dim annotation columns, and never the name being picked. Two details of that
  are load-bearing, both learned from the same symptom (`main` truncating to `m…` on the *widest*
  row of a table list while every shorter row rendered clean). The row list takes an **explicit**
  width, not `width_full()`: a percentage resolves against a definite parent width, and a `scroll`
  lays its child out against max-content available space, so the percentage quietly became "as wide
  as the widest row" and the rows never stretched to the box at all. And `row_width` carries
  `COMPLETION_SLACK_W` of air and rounds up, because a box sized to exactly its widest row puts
  that row on its own ellipsis boundary, where a sub-pixel disagreement between the measurement and
  the layout is a visibly wrong string.
  The **Ctrl+Enter run menu** is the fourth caret-anchored overlay and obeys both halves the same
  way, through `editor_pane::run_menu_pos`: `run_menu` stores the caret's line-bottom in content
  coords (via `content_x_of`, the gutter the highlight boxes use — `COMPLETION_GUTTER` is an
  under-estimate the suggestion list hides behind its own padding) and the style closure subtracts
  `ed.viewport`, then keeps the panel inside `content_x + vp.width()` — the same fold
  `statement_line_boxes_at` clamps to, rather than `area_w`. Horizontally it **flips** left of the
  caret, so the menu stays beside the statement it is about to run; vertically it **clamps**, since
  flipping would need the caret line's top edge and covering a line beats a jump. The flip alone is
  not enough: an anchor already past the fold (a caret scrolled out to the right) flips to somewhere
  still past it. `RUN_MENU_W` is one constant for the panel's `min_width` and the placement's
  arithmetic — comparing an unscrolled anchor against a width the panel wasn't drawn at is what cut
  the menu off at the editor's right edge on a long line.
- **The floem editor owns its document once mounted, and the sync is one-way — writing the `query`
  signal from outside is not merely invisible, it is lost.** Every edit runs
  `query.set(doc.text())` from the editor's `.update` callback (`editor_pane.rs`), and there is no
  effect pushing `query` back into the doc, so a write from outside a mounted editor changes nothing
  the user can see and the next keystroke overwrites the signal back from the doc. Replacing a tab's
  text from outside — reload from disk — therefore bumps `Tab::reload_gen`, which is part of the
  `editor_area` `dyn_container` key in `lib.rs` (`(active id, is_flashing, reload_gen)`), and the
  pane remounts on the new text. That key reads the tab out of `tabs` with `with_untracked` and only
  *then* tracks `reload_gen`: tracking the whole vector there would rebuild the editor every time
  any tab was opened or closed.
  A remount is the right answer only when the *whole document* is being replaced by something the
  user cannot have been mid-edit in. For an edit — anything that should be undoable and should keep
  the caret — the write has to reach the mounted editor instead, and the way in is a
  request-and-clear signal on the tab that the pane consumes (`Tab::format_req`, `Tab::jump_offset`).
  This is the shape the palette's "Format Code" now uses. It is worth knowing what it did before,
  because that is the failure mode: it called `sqlfmt::format_sql` itself and wrote the result into
  `t.query`, which nothing read back, so the command silently did nothing at all — and the bug was
  invisible in review, because the line that "applies" the format looks exactly like the line that
  would work if the sync went both ways. It also meant two formatters, so the palette could drift
  from Ctrl+Alt+L; `format_req` reaches the same `editor_pane::format_editor`.

## Popup menus (`menu_panel`)

Custom themed overlays, not Floem's native `Menu` (native renders OS-styled, clashes with the dark
theme). `menu_panel(entries: Vec<MenuEntry>, close)` takes `Action`/`Sub`/`Separator` entries and
renders the themed panel; the caller positions it absolutely. Used by the schema right-click menu
(`context_menu_overlay`).

- **Separators are tidied by the panel, not the builder** (`tidy_separators`, applied first thing in
  `menu_panel` and by `menu_panel_height` so the measured panel matches the drawn one): leading and
  trailing separators are dropped, a run collapses to one rule, and each submenu's own children get
  the same treatment. A builder pushes a group's separator *before* it knows whether the group has
  any entries — the column menu pushes one and then asks `field_entries` whether Edit column and
  Drop are offered, and on a **view's** column neither is — so the alternative is every conditional
  arm remembering to push its rule afterwards. It shipped as a rule with nothing under it: an empty
  section between "Copy qualified name" and AI Explain.
- **Nested submenus**: a `Sub` entry hover-expands a child `menu_stack` anchored to the parent row's
  right edge. The **keyboard** stops at one level (`MenuLevel`/`MenuSub`), which is as deep as any
  menu in the app goes; one flat pair of cursors is what lets `menu_key` drive whichever level is
  open without walking a tree it would then have to keep in step with the views.
- **The open submenu is drawn at the root of the window, not under its row** (`submenu_layer`, the
  last element of `workspace`'s root stack, after even `popup_menu_overlay`). It has to be: nested
  under its row it was painted but never hit-tested whenever it flipped left or shifted up, because
  Floem grows a parent's hit area rightward and downward only (see *Floem 0.2 gotchas*). It opened,
  it drew, and it answered neither hover nor click. That went unnoticed for as long as it did
  because it bites only near a window edge, and it surfaced on the ER diagram's export menu, which
  is anchored at the right end of a toolbar and so flips on any window narrow enough.
  A `Sub` row therefore builds no panel. It publishes `OpenSubmenu { entries, row, level, close }`
  into the `hoisted_submenu()` channel — a thread-local signal on a **detached scope**, since what
  publishes is a row inside a panel that is disposed the moment the menu closes — and the layer
  draws from that. Publishing is a `create_effect` on `open_sub`, not something `PointerEnter` does,
  because the keyboard opens submenus through the same signal (`MenuAct::Open`); one place, both
  ways in. **Clearing is `menu_panel`'s job, not a row's**: every row's effect re-runs on every
  change, so a row that also cleared would race the row that is opening. `menu_panel`'s effect fires
  on `open_sub == None`, which also sweeps up a stale submenu as it opens; `workspace` clears from
  the other side when both menu channels go empty, which is the click-away case where the panel's
  whole scope is dropped and its effect never runs again.
  Two things depended on the submenu being a view-tree descendant, and both survive the hoist:
  dismissal (the panel absorbed its children's pointer-downs — `menu_stack` does that for the
  hoisted copy too, being the same view) and the parent row's highlight (the keyboard cursor sits on
  that row while its submenu is open, and the cursor wears the hover fill).
  The layer is out of flow and **shrink-wrapped to the panel**, deliberately not a full-window
  sheet: a full-window layer would claim every pointer event in the window and swallow clicks meant
  for the app underneath.
- **Hover intent (no timers)**: entering a leaf clears `open_sub`, entering a submenu row sets it;
  nothing closes on leave. The submenu is flush with the panel's right edge, so a diagonal move never
  crosses a gap — the close-on-diagonal problem is avoided structurally.
- **Dismissal**: the panel `on_event_stop`s its own pointer-downs, so the root "pointer-down anywhere
  closes" handler (in `workspace`) fires only for outside clicks. Escape and any action also call
  `close`. A hoisted submenu is a sibling at the window root, not a descendant, so the panel absorbs
  nothing on its behalf — it stops its own pointer-downs, `menu_stack` carrying that
  `on_event_stop` for both copies because it *is* the same view.
- **Edge-flipping**: submenus flip left past the right edge and shift up past the bottom — from the
  parent row's window position (`on_move`/`on_resize`) + the live `window_size()` global (set from
  `workspace`'s root `on_resize`). `popup_menu_overlay` flips the whole panel the same way at the
  cursor. Only the width is a flat estimate (`SUBMENU_FLIP_W = 210` for a submenu; the popup uses the
  panel's real `min_width`); the height both ask for is `menu_panel_height`, which sums the entries
  that will actually be drawn — 30.5 per action row, 9 per separator, plus 14 — because counting
  separators as full rows shoved an upward-flipped panel tens of px too high. Neither is measured
  from a laid-out panel, so there's no open-then-flip flicker.
  A submenu's own flip is `submenu_insets`, which is a **pure function with tests** rather than four
  lines inside a style closure, because every way it can be wrong is silent — a sign slip or an `x1`
  where an `x0` belongs still draws a submenu, just not beside the row that opened it. It pins an
  *edge* rather than computing a corner: flush to the row's right edge normally, and when there is
  no room, `inset_right` from the window pins the panel's **right** edge to the row's left one
  whatever the panel measures. The width estimate therefore only ever decides *which* edge to pin,
  never where — so being off costs at worst a flip that wasn't needed, never a gap between the row
  and its submenu, and never an open-then-measure-then-move flicker. Same for the vertical: a panel
  that won't fit below the row pins its bottom to the window's, with no height arithmetic at all.
- **Two menu channels**: the schema tree uses `ui.context_menu` (typed `CtxMenu`) +
  `context_menu_overlay`; everything else uses the generic `ui.popup_menu`
  (`RwSignal<Option<Vec<MenuEntry>>>`) + `popup_menu_overlay`. Both overlays live at the workspace
  root (window coords) and close on the root pointer-down. **Both render through `menu_panel`**,
  which is why everything below is stated once and holds for every opener. (It said "all fourteen"
  until the Live Monitor's Export made it fifteen. The count was never reproducible — the openers
  are *logical* menus, not `popup_menu.set` sites, several of which are helper-driven — so it is
  gone rather than incremented: the sentence needs "all of them", not a number that silently rots.)
- **Every schema-tree menu is a subsequence of one skeleton, and `Drop` is always the last entry in
  its menu.** `overlays::context_menu_overlay`'s `build` closure matches on `CtxKind` — one arm per
  kind of row — and each arm emits the same five groups in the same order, separated by
  `MenuEntry::Separator`, so an action sits in the same place whatever was right-clicked:
  **Open** (what a double-click would have done), **Read** (`Copy name`, `Copy qualified name`,
  then what the node can *show* you — `Properties`, `Live monitor`, `Show diagram`, `Generate DDL` —
  closing with
  `Refresh`), **Tree state** (`Favorite`, `Colour ▸`, `Hide`, which act on the row and not on the
  object), **Write** (`Create`/`Edit`/`Import`/`Triggers`, with the entries that can't be taken
  back **last** inside the group and coloured `theme::error`), and the `AI Explain` row every menu
  ends with, appended outside the `match`. The write group's ordering is the load-bearing half: the
  row the cursor lands on after a right-click must never be the irreversible one, and two menus
  broke that before the skeleton was written down — the key/index menu opened straight onto
  `Drop index`/`Drop foreign key` with its edit entry (then labelled `Edit table`, now named for
  the row) *below* them, and the column menu's `Drop` was rendered in the same colour as the
  `Edit column` directly above it. A new entry joins its group rather than the end of the list, and
  a new arm reads the skeleton, which is stated as a comment
  block immediately above the closure: an arm is written and reviewed one arm at a time, and
  nothing else in the file says what the order is.
  **`overlays::menu_order_gate` holds the whole claim**, in four tests over the module's own source,
  bounded by the `let build:` binding and the `AI Explain` row pushed outside the `match`. The
  ordering can't be reached through `build` — it closes over a `Ui` — and it doesn't need to be: the
  order is a property of the *source*, because a dialect or a read-only connection can only **omit**
  an entry, never move one, so one pass covers every engine and every permission state at once,
  including engines that don't exist yet. `nothing_follows_an_irreversible_entry_in_its_own_menu` is
  checkable at all because every destructive entry marks itself
  `action_colored(…, theme::error, …)`, the same fact the menu shows the user, and
  `the_error_colour_marks_the_drops_and_truncate_and_nothing_else` pins that the marking is on
  exactly `Drop`, `Drop foreign key`, `Drop index` and `Truncate` so re-colouring can't weaken it
  silently. `every_menu_is_a_subsequence_of_the_skeleton` carries the five-group claim: `group(label)`
  is the skeleton as data, and an **unknown label fails** rather than being skipped, which is the
  load-bearing part — a thirteenth entry can't be added to any menu without placing it in a group
  first, which is the drift the comment was written to stop and could not.
  `drop_is_the_last_entry_before_ai_explain` pins the one position that isn't a matter of taste, in
  both halves: every menu that writes ends on its `Drop` (`Database`, `Schema` and `ObjectGroup` have
  nothing to drop, asserted by name), and nothing but `AI Explain` follows the `match`.
  The two deviations the shipped code already had stand, and the gate tolerates them **because they
  stay inside their group**, which is recorded at `group` rather than quietly excused: the database
  arm pushes `Collapse all` after `Refresh` where the skeleton closes the read group with `Refresh`,
  and the table arm's write group is `Import` → `Edit table` → `Triggers` where the skeleton lists
  Create/Edit/Import/Triggers. Order *within* group 4 is not checked; `Drop` being last is.
  Four things the scan has to get right, each of which made it wrong first: comments are cut out of a
  constructor's span (one entry explains its own label with `"(0)" reads as a broken count` two lines
  above the label, and the scan read the comment as the name); a label is read only from the span
  *before* `move ||`, which is what makes a twelve-line window safe; `let label = if … {"A"} else
  {"B"}` above a constructor is resolved backwards, since an empty label would have excused
  `Favorite` and the key row's `Edit` from the group check; and only entries pushed into `entries`
  count, because the colour swatches are `MenuEntry`s too — with `entries.extend(create_submenu(…))`
  counted as the `Create ▸` entry it is, or the two arms that write through it would look as though
  they stop at the read group.
  The Table arm's `Truncate`/`Drop` name the **scale** of what they delete when a row figure is
  already in `ConnNode::stats` (`stats::truncate_prompt`/`drop_prompt` decide the words and whether
  a figure is worth naming). It is read, never fetched: the menu is built on the right-click, so a
  round trip there would either block it or land after the modal is already up.
- **Keyboard operation lives in `menu_key`.** The panel is a `focus_root`, so it took focus and
  answered Escape from the start — but nothing moved a cursor and no row was marked, so a menu
  opened with Enter from a ringed button could only be finished with the mouse and read as though
  it had never taken focus. Up/Down step `menu_stops` and **wrap** (the swatches' rule, not the item
  list's: a menu is short and its ends read as adjacent), Home/End jump, Enter/Space runs the row and
  closes, Right/Enter opens a submenu and lands on its first child, Left comes back out, and Escape
  closes an open submenu **before** the menu so "back" never skips a level. Two things the cursor
  must not do are why `menu_stops` exists rather than a range: resting on a **separator** makes Down
  look dead, and resting on a **disabled** row offers an Enter that silently does nothing. The
  pointer moves the same cursor (each row's `PointerEnter` sets it), so there is never a second
  highlight disagreeing with what Enter would run. `menu_key` takes only signals, so the whole
  decision asserts without a window (`widgets::menu_key_tests`).
- **A menu raised by a *button* must set `popup_anchor`.** With it `None` the panel opens at
  `last_mouse`, which is right for a right-click and wrong for a button: reached by Tab and pressed
  with Enter, the pointer is wherever it was left, so the menu opened across the window from the
  control that raised it. `table_designer::suggest_chevron` anchors to its own ring wrapper —
  `ViewId::layout_rect()` is already in window coordinates, the frame `PopupAnchor` is stated in.
  The grid's toolbar dropdowns and the status-bar segments were always right: floem's `on_move`
  fires during **layout** with the view's window origin, not on pointer movement, so what they
  anchor to is the widget and not the cursor.
- **`popup_anchor` carries the menu's *identity*, not only its placement — so an opener must write
  it immediately before `popup_menu`.** One channel serves eleven openers across six modules
  (`connection_form`, `editor_pane`, `grid` ×5, `lib`'s status bar, `monitor_view`,
  `table_designer::suggest_chevron`, `tabs`) and nothing in it says who filled it, so the grid
  toolbar's AI / Copy / Save icons ran "dismiss, then open" unconditionally and a second press
  rebuilt an identical panel instead of closing it. The menus that already toggled could only half
  lend their answer: the schema panel's eye and gear, the connection switcher and the tab selector
  each render their *own* menu and so keep a private `RwSignal<bool>`
  (`db_menu_open`/`schema_menu_open`/`conn_menu_open`/`active_db_menu_open`), which is available
  only to a trigger that owns its panel. **`widgets::menu_anchored_at(open, anchor, mine)` is the
  answer for a trigger that shares the channel**, comparing `popup_anchor` against the
  `PopupAnchor` it would set itself — which is why `PopupAnchor` derives `PartialEq` and why its
  rustdoc calls that derive load-bearing rather than a convenience. It is self-invalidating **only
  because every opener overwrites the anchor as it opens**: there is no separate flag to go stale
  and nothing for the other ten to reset. An opener that fills `popup_menu` without setting
  `popup_anchor` first therefore hands its menu to whoever opened last, silently.
  **`widgets::popup_anchor_gate` is what catches that**, and it reads the source, because the sites
  are not greppable by one name — `overlay.popup_menu.set(Some(…))`, `gs.popup.set(Some(…))` and
  `table_designer`'s local `popup`/`anchor`. Walking each file of the crate in order, every write
  that *fills* the channel must have an anchor write between it and the previous fill: a stand-in
  for "in the same opener, before it", which holds because openers don't interleave. `rustfmt`
  breaks the long ones at the `.`, so the scan joins a continuation onto its owner — getting that
  wrong reported two correct openers as offenders. Deliberately weak, like `shortcuts`' `KEY_FILES`
  and `core/tests/doc_coverage.rs`, and it asserts a floor on how many openers it found so a rename
  can't make it pass by seeing nothing. Folding the pair into one `open_popup(anchor, entries)`
  constructor, so the anchor *cannot* be omitted, is still the better end state and is a change to
  make with the app running.
  `open` (is the channel non-empty) is checked **before** the anchor, which is the reason this is a
  named function rather than an inline `&&`: closing clears `popup_menu` but leaves `popup_anchor`
  naming the last opener, so an anchor-only test reports the menu still up after Escape and the
  next press closes nothing instead of opening. Six tests in `widgets::menu_key_tests` pin it,
  including `a_dismissed_menu_is_no_longer_owned` and `a_cursor_menu_belongs_to_no_trigger` — a
  right-click menu sets the anchor to `None` on this same channel. Each trigger states its anchor
  **once** (`grid_toolbar`'s `anchor_below`, `status_menu_seg`'s `anchor_here`,
  `suggest_chevron`'s `anchor_now`) because the value that *places* the panel is the value that
  *identifies* it: written twice, a pixel of drift would open the menu correctly and silently refuse
  to toggle it shut. Recomputed on each press rather than remembered, so it is the *current* rect
  that must match — `suggest_chevron` sits in a scrolling modal body, and scrolling with its menu up
  moves the chevron out from under it, at which point reopening at the new position is the better
  answer than closing. Which way the test fails matters more than its exactness.
  A tag beside the channel is what this replaced, and why it isn't the pattern: the status-bar
  segments carried a `menu_owner: RwSignal<u8>` written only by the segments themselves, so it went
  stale the moment anything else filled the channel — open a segment's menu, right-click a grid
  cell, press the segment again, and it closed the cell's menu instead of opening its own. All three
  surfaces ask the anchor now.
- **The app's menus are mutually exclusive, and the list of them is `widgets::MenuFlags`** — the
  seven `MenuId`s (`Popup`, `Context`, and the five panel-owned flags: the schema eye and gear, the
  connection switcher, the active-database selector, the Server Activity clock), gathered by
  `MenuFlags::of(&ui)` and closed by `close_except(keep)`. A trigger has to enforce this *itself*,
  because it absorbs its own pointer-down — it must, or the root's dismissal would close the menu the
  click is about to open — so the root handler never runs for it. **Both halves of that bargain are
  the trigger's**, and until recently two of the five click-opened menus had neither: the connection
  switcher (`lib::header`'s `switcher`) and the QUERY toolbar's active-database selector
  (`editor_pane`'s `db_selector`) registered only `on_click_stop`, which floem turns into an
  `EventListener::Click` handler and nothing else — so the root's `close_except(None)` ran first,
  closed the menu, and the `Click` behind it reopened it, and neither could be shut from the control
  that opened it. Both now carry
  `.on_event_stop(EventListener::PointerDown, widgets::menu_trigger_press)`, which is what makes the
  premise above true rather than nearly true. Absorbing the press is not free, though: it transfers
  the mutual-exclusivity duty from the root to the trigger, so each also calls
  `close_except(Some(MenuId::Connection))` / `close_except(Some(MenuId::ActiveDb))` in its own click
  handler — the shape the schema eye, the gear and the activity clock already had. Those three were
  the only ones making the call, and the two that weren't were exactly the two that weren't
  absorbing, which is why the gap stranded nothing on screen and nothing pointed at it. The
  selector's call sits **after** its "nothing offerable" early return, so an inert trigger closes
  nothing; its absorb is unconditional, because the root's dismissal is about the *other* menus and
  pressing a dead control should not close one elsewhere. `query_pane` carries a `menus: MenuFlags`
  on `QueryPaneParams` to make that call possible at all — `editor_pane` has no `Ui`.
  **`widgets::menu_trigger_gate::every_click_opened_menu_closes_the_others_itself` is what holds it**,
  a sibling of `popup_anchor_gate` and a source scan for the same reason: the thing under test is a
  set of call sites. It reads every `src/*.rs` in the crate with the test modules cut off and asserts
  each click-opened `MenuId` — `SchemaEye`, `SchemaGear`, `Connection`, `ActiveDb`, `ActivityClock`,
  but not `Popup` or `Context`, which are opened on `SecondaryClick` where the root dismisses on the
  press and the opener runs on the release, one gesture — appears in a `close_except(Some(…))`
  somewhere, in any of the three spellings the crate uses for the path (`crate::widgets::MenuId::`,
  `widgets::MenuId::`, bare `MenuId::`). Deliberately
  weak: it counts `close_except(Some(` and `menu_trigger_press` registrations against the number of
  click-opened menus so a rename can't make it pass by finding nothing, but which site is which is
  not checkable from source. That list was written out three
  times in three files, and the third one added a flag the other two never learned about: opening the
  activity clock's interval dropdown and then clicking the schema tree's eye left **both** on screen.
  A stranded dropdown is not merely visible — its `focus_root` stays registered, and
  `innermost_focus_root()` being `Some` makes every newly opened query tab decline the keyboard.
  `close_except` is guarded per flag (`RwSignal::set` never dedups, and an unguarded write re-runs
  every style closure reading it), the workspace root passes `None` so a pointer-down keeps nothing,
  and `closing_leaves_exactly_the_one_menu_that_asked_to_stay` walks every id. Add a new dropdown by
  adding a variant and a field here, not a fourth copy of the list.
- **The panel owes an absorb too, and it is the same fact read from the other end.** The root closes
  every menu on any pointer-down, and floem delivers `Click` on the way *up*, only to a view that
  still exists — so a panel that does not stop its own `PointerDown` is torn down by the root on the
  press and the row's click lands on nothing. The menu opens and choosing an item does nothing at
  all, which is worse than the trigger bug above because the surface still looks alive.
  `on_click_stop(|_| {})` on a panel is not this: it stops a different event, arriving too late.
  Widening the root from five hand-written flags to `close_except(None)` put the last two panels on
  this hook, and they were the same two as before — `conn_menu_overlay`'s and
  `active_db_menu_overlay`'s, the only click-opened bodies that had never needed the absorb because
  the root had never closed them. Both now carry the bare `|_| {}` form, which is deliberate and not
  `menu_trigger_press`: a click on a menu row is a gesture within something the keyboard may still
  legitimately own, so `keyboard_nav` must survive it. **`widgets::menu_panel_gate::`
  `every_click_opened_menu_panel_absorbs_its_own_pointer_down` holds this half** — a third source
  gate, for the third time because the thing under test is a set of call sites. It slices
  `overlays.rs` at its column-0 `fn` headers and asserts each of the five click-opened menus'
  overlay bodies mentions `EventListener::PointerDown`; `popup_menu_overlay` and
  `context_menu_overlay` are out of scope for the reason they are out of `menu_trigger_gate`'s, and
  because their body is `menu_panel`, which carries the absorb once for both.
- **A trigger says it is open in the accent** — `widgets::menu_icon_color(open, hovered)`, one
  function for the schema panel's eye and gear, the Server Activity clock, and the results strip's
  copy, download and AI icons, each of which had spelled its own two-arm hover match. **Open
  outranks hover**, which is the ordering the whole rule turns on: the pointer is still on the icon
  it just clicked, so a hover that won would paint "this menu is open" and "you are about to open
  this menu" the same colour for as long as the menu is up. The trigger is the only part of an open
  dropdown in the user's eyeline — the panel drops *below* it — so a menu raised over a busy grid
  used to have nothing on screen naming what raised it. The grid's three ask
  `menu_anchored_at` **reactively** (`menu_is_mine_live`, a `.get()` twin of the `get_untracked`
  `menu_is_mine` the click path uses) so the style closure re-runs when the channel changes; the
  panels' four read their own `RwSignal<bool>`. The results strip's AI icon keeps its busy arm
  *first*: a request in flight makes the glyph inert, whatever the menu and the pointer are doing.
  The query pane's database selector follows the same order in its own colours — it rests in
  `bubble_claude_text` rather than `text_muted`, so it states the rule inline rather than calling
  the helper, and both halves (label and chevron) take the answer, the label by inheriting it from
  the stack (never by reading `db_hov` inside its `dyn_container`, which is disposed-signal
  territory — see the note there).
- **A trigger that toggles must also stop its own `PointerDown`**, and the two halves are not
  alternatives. The workspace root closes `popup_menu` on any pointer-down (`lib.rs`, the same
  handler that clears `keyboard_nav`), so a trigger that doesn't stop the press has its menu closed
  *before* the click arrives, and the click reopens it — down closes, up reopens, and the trigger
  never toggles however it decides. That was `suggest_chevron`'s bug, and it is a different one from
  the grid icons', which had the guard and no toggle: there the menu never closed at all. Guard
  without toggle re-opens what was never closed; toggle without guard closes what the click then
  reopens. The status-bar segments have carried both for as long as they have toggled; the
  connection switcher and the QUERY toolbar's database selector carried the toggle without the guard
  until they were given `menu_trigger_press` (above), and behaved exactly as this says — every press
  an open. Stop it with
  `widgets::menu_trigger_press` and not a bare `|_| {}` — swallowing the press also swallows the
  root's `keyboard_nav` clear, which that handler is there to repay (see *`widgets::keyboard_nav`*).
- **Not every menu can be *opened* from the keyboard**, which is a separate thing from navigating
  one. A menu on a ringed control can be — `suggest_chevron`, the Live Monitor's Export dropdown,
  and the grid toolbar's Copy / Save / AI dropdowns since the strip gained its ring — and the
  **schema tree** answers `Shift+F10` and the `ContextMenu` key on the row the nav cursor is on.
  The grid's cell and header menus, the editor's, the tab strip's and the connection list's still
  need a right-click.
  The tree's route is worth reading before copying it: focus lives on the tree **container**, never
  on a row, so a key arriving there knows neither which row it is about nor where that row is. Both
  ride on the per-row effect that already existed to scroll the cursor into view — `Nav::cursor_menu`
  is the row's own `CtxOpener` (the same closure its `on_secondary_click_stop` calls, so the two
  routes cannot offer different menus for one row) and `Nav::cursor_at` is where to open it.
  That point comes from **`on_move`**, which floem fires during *layout* with the view's window
  origin — not on pointer movement, and unlike `on_resize`'s rect, which is view-local and supplies
  only the height. It is the row's **content** corner, `origin.x + get_content_rect().x0`: a tree row
  spans the whole panel and the panel is flush left, so every row's own x is 0 at every depth and a
  menu anchored to the box hugged the window edge. The indent that makes the tree a tree is the row's
  `padding_left`. Both geometry signals are read *inside* the cursor guard, so only the cursor row
  subscribes and a scroll that moves it refreshes the point.
- **An open context menu marks the row it acts on** — `Nav::menu_row`, painted by
  `schema_tree::menu_mark` as a 1px rule above and below in `theme::row_menu_edge`. A menu is a panel
  floating clear of its row, and once the pointer is on the menu nothing on screen said which
  database, table or column the Drop was about. Not the nav cursor's highlight: a right-click
  deliberately does **not** move the cursor (`resume_cursor` — a cursor that exists is the user's), so
  this is a second, shorter-lived mark. Set by `marking_opener`, which wraps the row's `CtxOpener`
  where it is *built* rather than at the click, so the `Shift+F10` route marks too; cleared by an
  effect watching `context_menu` go to `None`, which covers the closes the tree's own code never sees.
  Two details are load-bearing. It is a **border, not an `outline`**: floem strokes a per-side border
  *inside* the view's rect (`paint_border`: top at y = 0.5, bottom at height − 0.5), so nothing bleeds
  onto the neighbouring rows and no `z_index` is needed to keep their hover backgrounds off it, while
  an `outline` — which floem inflates outward — would have needed one; and taffy sizes the **border
  box**, so `height(TREE_ROW_H)` is unchanged and the rule costs 2px of content box, not a layout
  shift. The key/index leaf is the only row outside the nav sequence, so it carries its own
  `key_row_menu_key` (a prefix that never reaches the persisted expansion set) and calls `menu_mark`
  itself — a marker that skipped the one row kind with a menu but no cursor is a marker the user
  learns not to trust. `every_row_key_family_owns_its_prefix` guards the shared key space.
- **A menu the keyboard opened gives focus back when it closes** — `widgets::set_menu_return`, set
  by the opener and **taken** by `menu_panel` as it builds, so the slot lives only between the two
  and a later menu cannot inherit a stale return. Folded into `close`, the path Escape and every
  action take. Gated on `keyboard_nav` because it is only wanted there: after a click, moving focus
  to the control clicked would take the arrow keys away from whatever had them (the grid's own cell
  navigation), and a click-away dismissal sets the channel to `None` directly and skips it anyway.
  Without it the surface that raised the menu goes **keyboard-dead**: the panel is a `focus_root`
  with no other root above it in the workspace, so its teardown drops focus and the next key reaches
  nothing. Both the grid toolbar's F6 and the tree's Shift+F10 hit exactly that.
  **Handing focus back is a focus *event*, and a handler that re-seeds state on one is a bug waiting
  for a caller.** The tree's `FocusGained` seeded the nav cursor from the open table unconditionally
  — correct while the only way in was a click from outside, and wrong the moment the menu started
  returning focus to a tree that was already focused and already had a cursor: closing the menu moved
  the highlight to whatever table happened to be open, with no keypress asking, and the next
  Shift+F10 or Enter acted on that row. The rule is `schema_tree::resume_cursor` — a live cursor is
  the user's, and only an unset one is seeded — which is what the handler's own comment always said.
  Shift+F10 also **re-resolves** `nav.selected` against `visible_nav_rows` before opening, exactly as
  the arrows and Enter do: `cursor_menu`/`cursor_at` are published by the cursor row's own effect and
  never cleared, so a row that has gone (collapse its parent, or refresh the database) left a
  callable opener and a stale window point — the menu for an invisible object, over an unrelated row.
  **An icon closing its own menu deliberately does not arm the slot.** The slot is consumed by the
  next `menu_panel` as it *builds*, and a toggle-shut builds no panel, so a return armed there would
  sit in the thread-local waiting for the next keyboard-opened menu anywhere in the app to collect
  it. `grid_toolbar`'s `close_mine` hands the keyboard back directly instead — `focus_icon`, by
  tabindex and deferred, because the strip may have been rebuilt by the action just run and floem's
  focus request has no existence check — and still only when `keyboard_nav` is true.

## Data grid (results grid)

`grid_view` (in `grid.rs`) is built around `GridState` — a `Copy` bundle of `RwSignal`s created once
per result set and threaded into every cell/handler. It holds column widths, the selection
(`active`/`anchor` in **display** coords so selection stays put visually on sort), the display→data-row
`order`, the value-viewer/freeze/edit toggles, the `dirty` edit map, and `vp`/`scroll_to`/`focus_id`
for keyboard nav.

- **Two panes** side by side (`h_stack`): a **frozen pane** (row-number gutter + optional frozen
  column) and a horizontally-scrolling **data pane**. Rebuilt by a `dyn_container` keyed on
  `(sort, frozen)`. **Freeze is per-column, any column**: `gs.frozen` holds the frozen column's
  *absolute* index, set from the header right-click menu (no toolbar button). The data pane renders
  `data_cols` = `(0..ncols)` minus the frozen index (an `Arc<Vec<usize>>`); cells keep their
  *absolute* `ci` so selection/resize/sort stay consistent. Frozen pane width = `GUTTER_W + widths[frozen]`.
- **⚠️ Scroll-sync rule (cost a hang):** a scroll view must **never both read and write the same
  offset signal** — it re-enters its own layout and hangs the UI thread. Strict one-writer/one-reader:
  the **data pane writes `vscroll`** (`on_scroll`) and reads `gs.scroll_to` (keyboard channel); the
  **frozen pane reads `vscroll`** (its `scroll_to`), has **no `on_scroll`**, and blocks its own wheel
  (`on_event(PointerWheel, |_| Stop)`).
- **Column widths** (`gs.widths`) are estimated from content on load; the header's `col_resize_handle`
  drags to resize (moving-view trick) and double-clicks to auto-fit. Cells read `gs.widths` in
  `.style()` so resize is live. Every cell/header uses `flex_shrink(0)` so the row overflows (enabling
  h-scroll) instead of squeezing.
- **Selection**: click sets `active`+`anchor`; `PointerEnter` while `selecting` extends the range
  (drag-select, no capture); a gutter press selects the whole row and arms `row_selecting`, whose
  own `PointerEnter` extends by **rows** (`model::row_range_selection`) — a second flag, because
  sharing `selecting` would collapse a gutter drag to one column the moment it crossed a cell.
  Shift+click in the gutter extends from the current anchor. **A right-click inside the selection
  keeps it**, in the gutter and in the cells alike: collapsing to the clicked cell (which the cell
  menu used to do unconditionally) destroys the very block the menu is about, and the entries then
  describe one cell. Outside it, the press selects first, so a menu never describes something
  invisible. Copy (Ctrl+C / toolbar) emits TSV; a lone cell copies its raw value.
- **Paste (Ctrl+V / the cell menu) stages, it does not write.** Every pasted cell goes through the
  same `GridState::stage`/`stage_new` a typed edit does, so it lands as ordinary green edits and
  the write-back plan, the one-row safety net and Commit/Discard all apply unchanged — a paste is
  a batch of edits the user can still look at and throw away. The parse is
  `core::edit::parse_tsv_block`, **the exact inverse of `GridCells::tsv`**: split on newlines and
  tabs, no quote interpretation. A CSV-style reader here would be the obvious mistake — the copy
  side emits no quoting, so there is none to undo, and unquoting would silently turn a cell whose
  value genuinely is `"hello"` into `hello`. The cost is that a spreadsheet cell containing a
  newline arrives as two rows, which is the rarer wrong answer and a visible one. `plan_paste`
  lays the block over the grid: **one copied cell fills the whole selection** (that is how a column
  gets set to a constant), anything larger keeps **its own** shape from the selection's top-left,
  and everything is clipped to the display rows — pending new rows included, so a paste can fill
  rows the user just added. What falls outside, what lands on a read-only column, and what lands
  on a row marked for deletion are **counted and reported** in the same bottom bar a commit error
  uses (set *after* staging, since `stage` clears it), because a paste that discarded half a
  spreadsheet looks exactly like one that worked; a read-only column is skipped **in place**, never
  shifted, which would write one column's values into the next. Nothing is interpreted: a pasted
  cell reading `NULL` stages the four-character string, because that is what the copy side wrote
  and turning text into SQL `NULL` would be editing the user's data on their behalf.
- **What a cell *says* is resolved in one place, and it isn't the view.** `copy_selection` and
  `attached_rows` read the signals once into `grid_cells` — a `core::edit::GridCells` borrow over
  `rs`, `order`, `formats`, `dirty` and `new_rows` — and ask it for `tsv(rect)` or
  `attached(rect, cap)`; they contain no resolution of their own, and `displayed_cell_text` /
  `pending_cell_text` are gone. The reason is under `core::edit`: the rule went out one source
  short twice in the view, most recently without `format::apply`, so a `Timestamp` column attached
  the epoch integer the cell does not show. The **painter** is the exception and stays one:
  `data_cell`'s content `dyn_container` runs per cell per frame reading the signals one at a time,
  so it is the reference implementation `GridCells::text` is written against — the two must be
  changed together, and neither `grid_cells` nor its callers may become a per-frame path. Ctrl+C
  passes `formatted = false` and an attachment passes `true`, which is the *Copy formatted* entry's
  reason for existing.
- **Right-click menus** (generic `menu_panel` / `ui.popup_menu`): a header offers `Copy › CSV / JSON`
  of that column's values (`export_column_csv`/`_json`); a data cell offers `View`, `Edit` (editable
  cells only), a Copy entry whose scope and wording come from `edit::copy_scope` (**Copy** for the
  whole block when the right-click was inside a multi-cell selection, **Copy value** for one cell —
  the same word for three different amounts was the bug), `Set to NULL` (editable **and** nullable —
  stages `dirty` `None`), and
  `AI summary` (reveals the AI panel, prompts with source table + column for context). The
  **gutter** has its own menu (`gutter_menu`) rather than the cell one — `Copy`, the row actions,
  and the attach entry — because a row-number click has picked out no cell, and offering
  Edit field / Filter by this value there would answer a gesture about rows with actions about a
  column. Its row actions take **every selected row** (`selected_data_rows` → `set_rows_deleted` /
  `clone_row`) and count them in the label: the same menu naming five rows in one entry and acting
  on one in the next is how four deletions go missing unnoticed. The grid's app
  context (`source`, `db_nodes`, `connections`/`active_conn`, `popup`, `summarize`, `dismiss`, …) is
  bundled in `GridCtx`, threaded `results_section → results_view/multi → loaded_view → grid_view`,
  then stashed in `GridState` (whose `Rc` callbacks live in `RwSignal<Option<…>>` since it's `Copy`).
- **Menu dismissal**: grid cells consume the pointer-down (drag-select), so the root handler never
  fires inside the grid; cell/header/gutter click handlers call `gs.dismiss` (closes both
  `ui.popup_menu` and `ui.context_menu`, guarded).
- **Row view/edit panel** (`edit_row_panel`, replaced the old single-cell value viewer): the cell
  `View` item opens an **integrated in-flow bottom strip** (not a popup — `border_top` + panel bg, like
  the old viewer; the grid above shrinks) rendering the row as a **structured, per-field editor**, one
  row per column, over `core::rowjson`. Header = `Row {gutter#} · {table}` + a Save (✓) icon (shown
  only when the result has ≥1 editable column; otherwise it's a read-only row viewer) then Close (✕);
  a "Saving…" line shows while a save is in flight. Its **errors are not inline** — they go to
  `commit_err`, the same bottom bar a grid commit failure uses, because an inline message rendered
  under the field it came from was, on a JSON column, nested several scrolls down, so a save that
  didn't happen looked like one that did nothing. The JSON tree's own parse errors go the same way,
  and it keeps a **red outline** on the box meanwhile: the bar says what, the outline says *which
  field* — which is also why that editor still owns the error signal it mirrors into the bar (and
  takes back only its own message). The bar's **View** is offered per `text::hides_detail`, i.e.
  only when one-lining actually hid something; a parse error repeated verbatim in a modal is a
  button that does nothing. The panel is capped at **70% of the results area**
  (`edit_row_max`, measured on resize) and that cap sits on the panel's own column, so the field
  list is what shrinks and scrolls; the grid above drops its `min_height` floor while the panel is
  open, since flexbox honours a min-height over a sibling's size and the two together overflowed the
  area — which is what clipped the last fields out of reach of their own scrollbar.
  A scalar field is an `edit_field` bound straight
  to its `FieldSig::buf` with an explicit NULL toggle; a `json`/`jsonb` column gets the
  `core::jsontree` tree editor instead (click-to-edit leaves, collapsible containers, raw-text
  fallback for invalid JSON).
  **Save commits immediately** (its own path, not the staged `dirty` batch): `flush_fields` →
  `field_state` → `rowjson::update_changes` → `build_row_edits` (one `RowEdit` per base table, WHERE
  key from the *original* row) + a single-row `build_row_refetch` → the existing `CommitFn`; on
  success the row splices in place and the panel closes, on failure the message stays inline.
  **`flush_fields` is not optional**: clicking Save doesn't blur the field being typed into (floem
  moves focus on a pointer-down only for a `keyboard_navigable` view), so an editor holding a buffer
  of its own — the JSON tree's open leaf — has to be asked to commit before the write is assembled;
  a field that can't (invalid JSON) stops the write rather than letting the stale value through.
  This is the row panel's counterpart to `commit_grid`'s "flush any open in-cell edit first".
  Read-only fields (expression/`binary`) are shown for context but edits to them are rejected —
  **a key column is editable**, here as in the grid (`EditModel::editable` asks only whether the
  column maps to a base table) — the single exception being an implicit key, which maps to no column
  of the table and is left out when the model is built — which is why both re-fetches go through
  the one `edit::refetch_key`:
  the `UPDATE` keys on the *original* row, but the re-fetch has to look for the key it just wrote.
  Because this path is separate from the staged batch, its splice un-stages **only its own** changed
  columns (`model::drop_committed`) — a green cell edit elsewhere in the grid is still unwritten.
  State on `GridState`: `edit_row_open` / `edit_row_di` / `edit_row_saving` (errors have no signal
  of their own — see `commit_err` above). Real
  rows only (a pending new row is filled via inline cells); Esc closes it (after the find bar).
- **Inline edit writes back to the DB.** Per-column *provenance*: `schemaic-db` runs on
  **`mysql_async`** (sqlx's MySQL driver discards `org_table`/`org_name`/key flags), so each `Column`
  carries `origin: Option<ColumnOrigin>` — real `database`/`table`/`column` + `ColumnFlags`
  (pk/unique/not_null/auto_increment), or `None` for an expression (read-only). `analyze_edit` builds
  an `EditModel`: which columns are editable + each base table's WHERE key (schema PK first —
  authoritative for composite keys; else a fully-present unique NOT NULL index; else wire PK flags
  when schema isn't loaded; else, last, a backend-asserted implicit key — SQLite's projected
  `rowid` — which keys the write but is itself never editable). Flow: double-click an editable cell (or Enter) → inline editor; **Enter**
  stages into `gs.dirty` and paints the cell `grid_edit_staged()`; **Ctrl+Enter** / toolbar ✓ calls
  `commit_grid` → a `GridWrite { updates, inserts }` → the app's `commit_edits`. On success the app
  re-runs the query; on failure the error shows in the toolbar and green edits stay. No global "Edit"
  toggle. A read-only cell's double-click opens the value viewer.
- **Range selection is back, and it feeds the aggregates bar.** The state (`active`/`anchor`), the
  rect (`bounds`), the paint and `copy_selection` were always multi-cell aware; only the *input*
  had been gated off ("the grid has no multi-cell actions"). Shift+click, Shift+arrow and
  drag-select are live again — drag needs no pointer capture, just `gs.selecting` set on a cell's
  `PointerDown`, extended by each cell's `PointerEnter`, and cleared by the **whole grid's**
  `PointerUp` (floem dispatches a pointer event to the first hit child in reverse paint order and
  stops, so a release over the frozen pane, the header or past the last row never reached the data
  body's own copy — and the flag stayed armed with no button down, the selection following the bare
  cursor) **and by the double-click handler**, since floem's `DoubleClick` swallows the second
  `PointerUp`. `Del` drives the whole range to *one*
  state rather than flipping each row (on a mixed selection a per-row toggle both marks and
  unmarks, which reads as the key doing nothing) — that vote is `delete_vote`, and it is applied in
  **one** `del_rows.update` and one `dirty.update`: `toggle_delete` per row was two notifications
  each, so Ctrl+A then Del at the 200k row limit fired 400,000 of them and locked the window, on
  the two-keystroke gesture the feature exists to enable.
  **Which column the arithmetic is about is the *anchor's*** — the one the selection started on, so
  dragging from `price` across to `name` still reports `price`. It reads `gs.anchor` rather than
  `bounds()`, which is a normalised rect and has forgotten which corner you began at. A selection
  covering *every* column is a row selection (gutter click, Ctrl+A, the Ctrl+G jump) whose anchor
  column is column 0 — usually an id, whose sum means nothing — so those get counts only; a
  single-column result is exempt, since there covering every column is covering the one you meant.
  Those three rules are `selection_kind`, extracted and tested — they were inline in the effect
  while the arithmetic they gate had seventeen tests, so flipping `ncols > 1` to `>= 1` broke the
  exemption with nothing failing. The effect tracks `rs`, `order`, `dirty` and `new_rows` as well
  as the selection, and reads each cell **as the grid draws it**: tracking only `active`/`anchor`
  left the previous total standing under a sort or a commit splice, and a staged green edit was
  never in it at all.
  `grid_selection_bar` renders at panel level (like the find bar, so it can sit at the panel's
  edge) while `grid_view` computes it, and it lifts itself above `grid_error_bar` when that one is
  up: they coincide exactly when a bulk delete fails, which is when both have something to say. It
  sets `.pointer_events(|| false)` — it has no interactive content and otherwise swallowed every
  click on the cells it covers — where `grid_error_bar` keeps its events, owning a clickable
  **View**. **Every panel-level bar is cleared when the result stops being `Loaded`**, in one
  effect in `results_section`: their only writer lives inside `grid_view`, so a failed re-run left
  the previous total and a live-looking Go-to-row popup pinned to a panel saying "Query failed."
  Under a Run Everything strip that means the **shown** statement (`shown_panel_loaded`), not "any
  statement in the batch": only one grid is mounted, so switching from a loaded Result 1 to a failed
  Result 2 was the same bug one level along.
- **A failed batch statement reports in the error bar, not in the pane.** `grid_error_bar`'s first
  source is `batch_err`, a memo over the shown panel (`shown_panel_error`), ahead of `commit_err` —
  a failed statement has no grid mounted, so a commit error left over from another result tab would
  be describing something off screen. The pane keeps only a dim "Statement failed.", the way
  `grid_view` notes `Phase::Failed` while the editor bar carries a single run's message. It used to
  *be* the pane (`centered_msg(m, theme::error)`), and a server error is one long line: it rendered
  unwrapped across the middle of the window and out over the schema sidebar. A batch has no editor
  bar to fall back on — `run_all` sets the tab's `results` to `Idle` — which is why the panel bar is
  where this goes, **View** (`text::hides_detail`) and all.
- **A result says which database it came from, and the answer lives on the result.** The grid's
  stats line leads with `ResultSet::database` (`world · 100 rows · 15 cols · 1 ms`), stamped by the
  loader that knows the scope — `Db::fetch_query`, `Session::fetch_query` (its *pinned* database,
  which need not be the tab's current one) and `Db::run_batch`, which wraps its `on_result` sink
  once so neither engine's path can forget. **Not read from the tab**: a tab's selection moves, so a
  result outlives the database it ran under the moment someone changes the selector, and a label
  sourced from the tab would be wrong in exactly the case it exists to catch (the other half of that
  bug — a result landing in a rebound tab — was fixed by cancelling the run). On the result it is a
  snapshot by construction and survives a commit splice, which mutates the columns in place;
  a test pins that, because the label vanishing on first edit would strand it exactly when the user
  is writing to whatever it names. It is also **per result**, which a status-bar or footer line
  could not be: Run Everything renders a grid and a toolbar per statement. `None` (a connection with
  no default database) prints nothing rather than inventing a name, and the field names the
  statement's *scope*, not the origin of every row — a qualified `world.country` read while scoped
  to `sakila` still reports `sakila`. **The scope follows a `USE`**, in both `run_batch` and
  `Session::fetch_query`: it was computed once, on a method whose own doc advertises that a `USE`
  carries across statements, so `USE sakila; SELECT * FROM actor;` from a `world` tab really ran
  statement 2 in `sakila` and labelled it `world` — the label lying in exactly the case it exists to
  catch. `sql::use_target` reads it and is deliberately conservative: a `USE` it can't read plainly
  drops the label to `None`, which prints nothing, rather than carrying a name now certainly wrong.
- **A capped result says what it capped, when it can say it honestly**: the stats line reads
  `1,000 of ~4.2m rows (capped)` rather than `1,000 rows (capped)`. Three things have to line up, and
  `grid_view`'s `row_total` memo is where they do. The read must be capped
  (`ResultSet::truncated`); `grid_query` must be empty, because a spliced header filter re-runs a
  statement that is *not* `base_sql`; and `base_sql` must return the table entire
  (`intel::full_table_source`), or the table's estimate is a total this query never had. The figure
  then comes from `ConnNode::stats` via `stats::catalogue_key` + `SchemaStats::find`, requested
  through `SchemaActions::db_stats` and read reactively so it appears when the fetch lands — which is
  why the line is a `label` and not a `text`. **The ask tracks `SchemaUi::stats_gen`**, like the
  tree's size column: a schema Refresh resets every slot to `Idle`, the memo that prints the figure is
  live on that slot and dropped it immediately, and an ask that ran only at grid-build time meant
  `of ~2.84m` vanished from an unchanged on-screen result and never came back — unless the *opt-in*
  size column happened to be on and that database expanded, which is the tree's refetch and not this
  one. A repeated ask is free: the slot is the guard. `plural` still follows the rows
  actually *read*: they are the subject of the sentence, and `1 of ~4.2m row` is the wrong noun.
  The cap itself is unchanged and is still a **client-side stream cutoff**, not a `LIMIT`; this only
  says how much of the table went past it.
- **The RESULTS title bar carries Properties, Live Monitor and the editor-collapse toggle**, in that
  order and all tooltipped. Properties leads because it describes the table as it stands while the
  monitor watches it change — the same order the schema tree's Read group puts them in. Both act on
  the tab's source table and are gated on it *existing*, which is deliberately weaker than the row
  actions' `insert_target`: a table with no usable row key passes, and the monitor then answers "No
  row key for this table" rather than the button being silently dead. The same reasoning covers a
  view, whose properties panel says which figures an engine publishes for one.
- **Find (Ctrl+F) and Go to row (Ctrl+G)** are two popups sharing one anchor at the panel's
  top-right, and both are **split in the same way**: the bar renders at the RESULTS-*panel* level
  (`grid_find_bar`/`grid_goto_bar`, mounted in `results_section`) so it can sit at the panel's edge,
  while the work happens in `grid_view`, which is the only place that has the row data. Find is
  incremental on `find_query`; goto fires on a `goto_step` **nonce** the popup bumps on Enter,
  because a jump belongs to submit rather than to every keystroke. `grid_view` keeps at most one of
  the two open, as the editor does with its own pair — in **both** directions: the exclusion
  tracked `goto_open` alone, so Ctrl+F over an open Go-to-row left both mounted on one anchor and
  you typed into the one you couldn't see. Go to row resolves through the pure
  `model::goto_row_index` — 1-based, in **display** coordinates (the gutter numbers what is on
  screen, so "row N" means the Nth row *as sorted*), over the rows the gutter **numbers**: a
  pending unsaved row reads `*`, so counting them in made "row 101" land on a row showing no number
  and made the clamp stop one short of the last one that does. It
  **clamps** to the nearest end when the number is outside the grid: past the last row goes to the
  last, `0` goes to the first, and a number too wide for a `usize` clamps with every other overshoot
  rather than falling through to the not-a-number path. A row of 9s is how people ask for the bottom
  of a long result, and a silent no-op there can't be told apart from a broken feature, while
  overshooting is cheap to recover from — the gutter number and the row highlight say where you
  landed. **It accepts exactly the forms the grid prints**, which is why it isn't a
  `parse::<usize>()`: `human_count` writes `200k`, and typing that back returned `None` — the count
  on screen was a miss. `200k`/`1.25k`/`3m`/`1b` all read, in fixed point; separators are accepted
  for a count pasted from elsewhere (the app writes none). `None` is left for the only two cases
  that can mean no row: an empty grid, and input that isn't a number.
  The three decisions around it are **`model::goto_target`** — which row, the landing gesture, and
  the scroll column — so `grid_view` is a wrapper rather than a place any of them can quietly
  change. The gesture is `model::row_selection`, shared with the gutter click so the two can't
  drift: a divergence would also stop the aggregates bar reading the jump as a *row* and start it
  summing ids, which is why one test asserts that agreement across the crate boundary. `scroll_col`
  is **0**, not the active cell's column, so a jump doesn't also fling the viewport to the far
  right of a wide result. `goto_fires` is the first-run nonce guard (the effect is created whenever
  the grid is, and must not jump on its build run), and `one_bar_at_a_time` the find/goto exclusion
  — which is a **reactive** test over two signals in a `Scope`, the pattern to reach for when a
  rule is genuinely about signal propagation rather than about a value.
  **Closing either bar hands the keyboard back** (`focus_id.request_focus()`, on a true→false edge
  of the open flag). This is not optional and it is not the bar's job: Escape only flips the flag,
  floem then disposes the field's view and clears `app_state.focus` **silently**, and the grid was
  left focused on nothing — the next Ctrl+F reached nobody until the user clicked a cell. The bar
  can't do it either, being built a level up where `focus_id` doesn't exist, which is why the rule
  lives on the flag in `grid_view`. A new panel-level bar over the grid inherits this obligation.
- **Every control on a panel toolbar carries a tooltip too** — the schema panel's eye
  ("Show or hide databases") and gear ("Schema options"), the AI panel's new-chat and gear, Query
  History's trash ("Clear this connection's history…", named for what it clears, since a bare
  "Clear history" beside a per-connection list reads as all of it), and the terminal's
  open-DB-CLI / restart / gear. Server Activity had them already. Each is `.tooltip()` chained
  *after* the control's own `.style()`, for the reason the clock documents at `interval_button`:
  `.tooltip()` wraps the view, so a style applied after it lands on the wrapper while
  `on_move`/`on_resize` stay on the padded container inside — and that box is what the dropdown
  hangs off.
- **Every control on the results toolbar carries a tooltip**, and where it sits is a correctness
  rule. The eight are commit ✓, discard ✗, ＋, －, clone, ✦ AI, copy and save; before they were added
  nothing in the strip was labelled but the commit count, which is a bare number saying neither what
  it counts nor what pressing it does, and the glyphs doing the least reversible work (discard,
  delete, AI) are among the least self-describing. The tip goes on the **face, before
  `in_strip_button` wraps it**: `.tooltip()` allocates a fresh `ViewId` and it is the *wrapper* the
  ring registers and paints the focus outline on, so decorating the wrapper puts an id in the ring
  that paints nothing (the hazard `row_button` documents — see *Buttons are in the ring too*). The
  commit tip is built from that rebuild's count and busy flag rather than read reactively, since the
  `dyn_container` around it already keys on both and replaces face and tip together; the AI tip *is*
  reactive, because `ai_busy` dims its sparkle without rebuilding the block and a tip still offering
  to generate would be the only thing on screen disagreeing with the greyed glyph. Copy is
  deliberately **not** labelled Ctrl+C: that key copies the selection straight to the clipboard,
  which is a different action from the format menu the icon raises. `－` and clone keep their tip
  while dimmed and it names the selection, which is also the answer to why they are inert.
  `toolbar_sep` takes 8px of horizontal margin on top of the cluster's own 3px gap: the icons carry
  a padded hitbox and no visible edge, so a divider set at the plain group gap reads as *part of*
  the group beside it rather than the boundary between two.
- **The three dropdown icons toggle.** A second press on AI, copy or save closes the menu it opened
  instead of dismissing and rebuilding an identical panel — the mechanism, and the ordering rules
  that make it correct, are under *Popup menus* (`popup_anchor` carries identity).
- **Row actions: new / clone / delete.** Gated on a single writable table (`EditModel::insert_target()`;
  hidden for joins / read-only), committed in the shared `GridWrite` transaction (`commit_writes` runs
  **deletes → updates → inserts**, each exactly 1 row). A keyless SQLite table opened through its
  rowid qualifies as writable here too, and the rowid column stages nothing: it is never editable,
  so it is absent from a new or cloned row by construction rather than by a rule anyone has to
  apply.
    - **New (INSERT):** toolbar **"+ Row"** appends a blank pending row (`gs.new_rows`), rendered below
      real rows with a `*` gutter marker + faint green wash, first editable cell opened. Cells stage
      via `stage_new` (unset = server default; `Some("")` clears to default). Unset cells preview
      `<auto>`/`<required>`/`<null>`/`<default>` (from wire `auto_increment`/`no_default`/`not_null`).
      Tab/Enter hop cells (`advance_edit`).
    - **Clone:** right-click **Duplicate row** seeds a pending row via `add_cloned_row` (every editable
      column except auto-increment).
    - **Delete:** right-click **Delete row** or the **Del** key marks a real row (`gs.del_rows`) with a
      red wash; marking clears its staged edits. `build_deletes` keys each `RowDelete` by the table's
      `key_cols` + original values.
  Inserts/deletes change membership/order → those commits **full-re-run** the query (pure-UPDATE
  splices in place). A NOT-NULL-no-default omission or duplicate-key clone fails the transaction and
  surfaces the error — nothing half-applied.
- **A write that waits says so.** A commit can block indefinitely on another session's row lock,
  and the grid used to sit there silently until `innodb_lock_wait_timeout` (50 s) turned it into a
  bare error. Both write paths therefore `arm_wait_note` when they hand the write off: if it is
  still in flight `WRITE_WAIT_MS` later, `tx::write_wait_note` fills `commit_wait` and the
  panel-level bar (`grid_error_bar`, the same bar as a commit error — a write is either still
  waiting or has failed, never both) states the wait and, when exactly one of the user's own
  transactions could be the holder, offers **Roll back _tab_** straight into `rollback_tx`. That
  last part is why the note is worth having: Schemaic owns both transactions, so it can name the
  user's own second tab where every other tool can only hang. The bar is deliberately **not**
  bounded like `run_ddl`'s `lock_wait_sql` — a timeout is right for a modal that refuses every
  exit, wrong for a cell edit that could just have waited a moment longer. `commit_seq` is what
  keeps an earlier commit's timer from narrating a later one.
- **Type-aware headers** show `type_name` under the name (two-line, `GRID_HEADER_H`). A sorted column's
  name + chevron use `grid_sort()`; a column with selected cells gets a `grid_col_sel()` header
  background. **Key icons** (PK = gold key-round, single-col index = blue key-square, FK = purple
  key-square; colours shared with the schema tree via `key_primary/index/foreign`) come from
  `column_key_map`, cross-referencing the tab's `source` against the loaded schema (`db_nodes`). Only
  populated when the tab was opened from a table with schema loaded; arbitrary SELECTs get none.
  Nullable markers deferred.
- **Column virtualization.** Both rows (`virtual_stack`) *and* columns are virtualized: the header and
  every data row render only the columns intersecting the horizontal viewport (+ a small overscan)
  between two width-preserving spacers, so a wide table builds ~10-14 cells/row instead of all of them
  (a 100k×50 inertial fling stays smooth). The visible window is a `ColWindow` (`start..end` into
  `data_cols` + left/right spacer px) from a `create_memo` — it recomputes on scroll but, since memos
  dedup on `PartialEq`, only *notifies* (rebuilding header + row cells) when the visible column set
  changes, not every pixel. Header and every row read the **same** `win` memo, so the panes stay
  aligned. Invariant: `gs.widths` stays full-length and each row's total width = `sum(widths[data_cols])`
  (spacers make up the hidden columns), so `h_off`/`scroll_to` geometry is unchanged —
  `scroll_active_into_view` sums in data-pane space (excluding the frozen column) to match.
