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
    grid's `rs` signal and the shown panel's canonical `QueryState::Loaded` hold the *same*
    `Arc<ResultSet>` on purpose, so the post-commit splice mutates through `Arc::make_mut` at a
    strong count of 2; with the columns inline that deep-copied every arena (30 ms and ~160 MB at
    200k×50, on the UI thread, on the one path built to avoid a rebuild). `splice_rows` replaces
    only the column `Arc`s whose values actually changed, so an untouched column is never copied
    (29.6 ms → 1.8 ms at 200k×50, measured). `retained_bytes` sums what a result costs to *hold* —
    each column's arena plus its packed offset word per cell, as allocated — which is what the
    results strip shows on a kept result, and the reason a result of nothing but NULLs is still
    megabytes: the cost is per cell, not per character.
    **`ResultBuilder::take_chunk(capacity)` is the one way a load produces more than one result**:
    it closes off the rows pushed so far as a `ResultSet` and starts a fresh builder over the same
    columns, which is what lets an uncapped export reach the disk in blocks instead of holding the
    table. `finish` still *consumes*, because an ordinary load produces exactly one result; a load
    being written to a file as it arrives produces a sequence of them. The chunk's `elapsed_ms` and
    `truncated` are deliberately zeroed — both are facts about the whole load, and an uncapped
    stream is never truncated — so the caller stamps the total on whatever it reports. Four tests
    here pin the chunking directly, where it had only the indirect coverage of the db crate's
    streaming tests: the columns carrying forward with the builder left empty but *typed*
    (`take_chunk_cuts_the_rows_so_far_and_carries_the_columns_on`), `truncated`/`elapsed_ms` zeroed
    on the emitted chunk and not inherited by its successor
    (`a_chunk_claims_neither_truncation_nor_the_loads_elapsed_time`), the empty chunk a load whose
    rows divide evenly by the block size ends on
    (`taking_a_chunk_with_nothing_in_it_still_yields_the_columns`), and `capped_columns` not carried
    into the next block, which would report blank cells in a chunk whose cells are all there
    (`capped_columns_are_not_inherited_by_the_next_chunk`). Note that
    the **grid's** result is still materialised whole up to the row cap; it is the *export* that
    streams. `Column`/`ColumnOrigin`/`ColumnFlags` carry the
    write-back provenance the wire reports per column, and a binary column is unconditionally
    read-only (it can't round-trip through text). **A raw-bytes cell has exactly one rendering, and
    it lives here:** `binary_display(len)` → `<n bytes>`, with `is_binary_display` as its
    recognizer and `type_is_binary` / `Column::is_binary` as the question "is this column bytes at
    all". The three engines each used to answer differently — SQLite showed the size, MySQL
    `from_utf8_lossy`'d the bytes into mojibake, PostgreSQL handed over the text protocol's `\x…` —
    and the mojibake was a data bug rather than a cosmetic one: it *looks* like data, so a CSV or
    `INSERT` export wrote the replacement characters as the value and re-imported as the wrong
    bytes. SQLite's was the honest answer and is now everyone's.
    **What the wire carries is not what a type is, and `BIT` is the case that proves it**: MySQL
    hands a bit-field over as bytes, which put it on `type_is_binary`'s list, and a `BIT(8)` holding
    10 then rendered as `<1 bytes>`, was *withheld* from the CSV and JSON exports — the formats
    this app reads back — and was read-only into the bargain. A bit-field has a lossless text form:
    its number, which is also what MySQL accepts back. So it is off that list, `type_is_bit` names
    it, and `bit_display` reads the bytes big-endian the way the server wrote them (`convert_row`
    hoists that per column beside the binary mask, since only the column's type can say those bytes
    are a number). PostgreSQL's `bit` never arrived as bytes at all — its text protocol sends
    `00001010`, which was being reported as a byte count.
    **`bit_cell(bytes)` is what the MySQL loader stores for a `BIT` column, and the variant is the
    load-bearing part**: it is a `Value::UInt`, because a `Value::Str` reached `export::sql_literal`
    as the quoted `'10'`, and `'10'` assigned to a MySQL `BIT` column is not the number ten —
    `Field_bit::store(const char*, …)` takes the raw bits of the *bytes*, so it is `0x3132` = 12594
    on a `BIT(16)` and "Data too long" on a `BIT(8)`. A named function rather than two words in the
    loader's `match`, so the seam that broke is reachable from a test: `bit_value` was right and had
    a table of tests all along, and what was wrong was the variant its answer got wrapped in on the
    way to an exporter.
    `Column::is_binary` reads **two**
    inputs because neither covers every result: `ColumnOrigin::binary` is the authoritative wire
    flag but exists only for a table-backed column, so a `bytea` expression with no catalog
    provenance reached every caller as ordinary text until the type name was consulted too.
    Conversely nothing may act on the type name *alone* — a SQLite `BLOB` column is an affinity,
    not a promise, and may hold ordinary text — which is why every decision that discards a value
    (`export::dropped_binary_columns`, `pg::pg_cell`) requires the type and the value to agree.
    **`ResultSet::binary_columns` is a third input, and it is not a type at all** — it is the
    backend's own record that a value in this column *arrived* as raw bytes, set through
    `ResultBuilder::mark_binary` (SQLite's `fetch_query`, from `ValueRef::Blob`, which is an
    assertion rather than a heuristic). `export::dropped_binary_columns` ORs it into the type signal
    because on SQLite there are two shapes with no type signal to find: a blob living in a column
    declared `TEXT`, and any column with no `origin` at all — an expression, a join, a CTE.
    (The untyped column was already covered: `db::sqlite::declares_bytes` asks
    `schema::sqlite_affinity("")`, which is `Blob`.) Recorded per value and reported per *column*,
    since that is the grain everything downstream asks at, and a column marked here is still tested
    per cell against `is_binary_display` before anything is withheld — so a row of real text in a
    column that held a blob elsewhere is untouched.
    **The flag is computed once per result, never per cell.** `Column::is_binary` splits a type
    name and walks a keyword list; the read loops run up to the row cap times the column count, so
    both backends hoist it out — MySQL into a `Vec<bool>` before `ResultBuilder::new`, PostgreSQL
    into `pg::cell_kinds`, which now carries the binary flag and the numeric `NumKind` **together**,
    one `CellKind` per column, since both halves were derived from the same type name. Asked
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
    misses by construction and there is no write site to remember. Hosts the shared `SQL_KEYWORDS`/`FUNCTIONS`/
    `STMT_KEYWORDS` (the UI's completion + editor build on these). Also `join_condition` (FK-aware
    `JOIN … ON` auto-fill), `db_error_diagnostic` (positions a live DB error within the statement),
    `parses` (Tier-2 gate), and `select_output_names` (a projection's column names *in order*, or
    `None` when the statement alone can't say — what `ddl::pg_replaceable` reads).
    **`error_fix_range` decides what an AI fix is allowed to rewrite**, and it is a range question
    rather than a prompt one: a fix replaces text in the editor, so hand the model the buffer and
    every statement the user did not ask about comes back rewritten — which is what the error bar
    did, passing `0..sql.len()` unconditionally. One statement in the buffer keeps the whole buffer
    (the buffer *is* the statement, comments around it included); with several, the same
    `locate_db_error` that places the squiggle finds the offending token and the fix scopes to the
    statement holding it; an error naming nothing findable keeps the whole buffer, because guessing
    a statement would be a rewrite of the wrong one. **The token has to be unique in the buffer**,
    and that is the one place this parts company with the squiggle's lookup: `locate_db_error`
    answers with the *first* case-insensitive hit, which is right when it is handed one statement
    and wrong when it is handed a script — `near 'FROM t2'` names a word every statement has, so the
    fix scoped to the first one, a working statement that Accept would then overwrite while the
    broken one stayed broken. A token that appears twice identifies nothing, so `repeats_ci` sends
    it back to the whole-buffer fallback. `problems_in_range` is its companion for the
    editor's own diagnostics — every message touching a range, ordered, **deduplicated**, since the
    offline and DB-validated passes report an unknown column in the same words and the count reaches
    the user as "fix these 2 problems". Both of its arms are **half-open**, the zero-width one
    included: a point sitting on a statement boundary belongs to the range that starts there, and
    read inclusively it was reported for both neighbours at once.
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
  - `filter.rs` — the header filter/sort bar: a dialect-aware `sqlparser` **AST rewrite** that
    splices a `WHERE`/`ORDER BY` into the `SELECT` that produced the result and hands back SQL to
    re-run — so filtering covers the whole table, not the loaded page. `build_query` rewrites only
    a structurally simple, join-free, CTE-free single-table `SELECT` and degrades to `Ok(None)`
    ("not filterable") rather than erroring, because eligibility is the caller's question — a
    question it now asks `intel::simple_select_source`, which is also what SQLite's write-back
    asks, rather than answering inline.
    **`rerun_statement` is what the two affordances that re-run a result on screen ask** — the
    capped notice's "read N rows" and the export's `All rows` — and it composes *what* would run
    (`build_query`, or the base verbatim when the filter bar and the sort are both empty) with
    *whether it may* (`sql::rerunnable_for_export`), so the write guard sees the exact string that
    would be dispatched. It lives here rather than at either call site because a term a caller can
    delete is a term the suite cannot hold: the export menu's copy of the guard was deletable with
    everything green, and the read-more link had no copy at all (*Architecture invariants*, the write
    guard). An empty or whitespace base is `None` too, since `contains_write("")` is `false` and a
    bare "is it a write?" gate therefore says yes to nothing at all.
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
    **`staged_cell` is the one revert-to-original rule**, for a single cell: `Revert` when the value
    being staged *is* what was already there — drop any edit at that cell and don't count it — and
    `Stage` otherwise. The NULL normalisation is inside it rather than at the call site because it is
    the whole of the decision: a NULL cell *displays* as the four characters `NULL`, so comparing the
    display text made a NULL original read `Some("NULL")` and never equal a pasted NULL, turning
    "copy a column and paste it straight back" into a full staged rewrite and making `paste_report`
    announce `Pasted 300,000 cells` over a paste that changed nothing. A cell that is not there at
    all is `(None, true)`, which is what an out-of-range index silently meant in the view.
    `GridState::stage_many` is a loop over it and counts exactly the entries it answers `Stage` for,
    so the report and the dirty map cannot disagree about what landed.
    **The staged-cells → `RowEdit` grouping is here for the same reason, and it is the step a paste
    stresses.** `build_edits(model, rs, dirty)` folds a `DirtyCells` map — `(data row, result
    column) → new value`, `None` being SQL NULL, the shape `GridState::dirty` holds — into one
    `RowEdit` per (base table, data row) via `row_edit_for`, which builds the `SET` list and the
    `WHERE` key off the row's *original* values; `origin_column` is the one reader of a result
    column's real name on its base table. `GridState::build_edits` is now a thin wrapper over it. A
    typed edit stages a single cell, so every fixture this family has ever had carried one `SET`
    column and one key column, while a paste stages a rectangle — and grouping that rectangle is
    exactly what stands between it and `GridWrite::plan`'s 1-row safety net. The grouping lived in
    `GridState`, which no test can construct, so the chain paste → `dirty` → these edits → `plan` →
    `one_row_verdict` was untested end to end. The `BTreeMap` and `row_edit_for`'s sorted `SET` list
    are load-bearing rather than tidiness: a failing commit has to reproduce identically.
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
    staged edit, then the stored cell through `format::apply`. `tsv(rect, frozen)` is the clipboard's
    block and `attached(rect, cap, frozen)` is an AI attachment's column names, rows and pre-cap
    total — both emitting the selected columns in the order they are **drawn** (`visual_cols`) rather
    than in index order, because whoever receives the block reads it left to right and a copy across
    a freeze reached a spreadsheet transposed. One rule,
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
    **`visual_cols(ncols, frozen)` is the one definition of the order the grid draws columns in** —
    the frozen column, then every other column in index order. A frozen column keeps its *absolute*
    index on purpose, so selection, sort and resize stay consistent, which means draw order and index
    order disagree the moment `frozen` is `Some(f)` with `f > 0`. That is fine for a rectangle's
    *membership* and wrong for everything about adjacency or reading order: `plan_paste` extends a
    block along this order from the anchor, and `tsv`/`attached` emit along it. Walking the index
    range instead put the second value of a two-wide paste dropped on `email` into a frozen `ssn` —
    a column at the far *left* of the screen the user never pointed at, one `UPDATE … SET ssn` away
    from destroying it — and sent a copy across the freeze to a spreadsheet transposed. One list,
    because `grid::scroll_active_into_view` already sums widths with the same filter and a second
    spelling of the order is a second chance to disagree with it. The **single-value** paste is not
    an exception: it fills the *selection*, which is the set of cells already painted highlighted, so
    no translation applies. **What this deliberately does not fix**: a selection whose absolute range
    straddles the frozen column is visually discontiguous, so copy→paste of such a selection no
    longer round-trips. The ambiguity is in the selection model, not in either surface, and both now
    agree with what is drawn rather than with each other.
  - `export.rs` — CSV/JSON/SQL/Markdown/HTML/Excel export (incl. CSV formula-injection guard;
    Markdown pipe/backslash escaping; HTML entity escaping). **Rows arrive through a pull source,
    not as a `&ResultSet`**: `RowChunks::next_chunk` hands over one `RowChunk { rs, order }` at a
    time and `ExportFormat::stream_to` renders from it, returning an `ExportTally`. Every entry
    point took a whole result until the whole-table export landed, which made "export" mean
    "export whatever the row cap fetched" — and materialising two million rows to render them
    would hold the table twice over and run into `ResultSet`'s 512 MiB per-column arena ceiling,
    which does not fail but blanks the cells past it, so the file would quietly have holes in it.
    **Pull and not push, because JSON decides it**: its renderer holds a `serde_json` sequence
    serializer open across the whole array, borrowing the writer, and that is not a value a push
    API could store between calls — inverting it keeps the serializer a local in one function.
    `OneChunk` is a source of exactly one chunk (what the grid's own export uses) and `PullChunks`
    adapts a `FnMut() -> io::Result<Option<ResultSet>>` — the app's `rx.blocking_recv()` —
    materialising natural order into a **reused** `Vec<usize>`, one allocation for the whole
    export against six renderers that would each have needed a second code path. `OneChunk`
    yields **even for an empty result**, which is load-bearing rather than an edge case: CSV,
    Markdown and HTML take their header from the first chunk, so a source that yielded nothing for
    a zero-row result would write an empty file where the old code wrote a header. One empty chunk
    and no chunk at all are deliberately different things
    (`an_empty_chunk_carries_a_header_and_no_chunk_carries_nothing`). Each `export_*_chunks` is
    now the *only* implementation: every `*_to<W: io::Write>` form is that function over a
    `OneChunk` and `ExportFormat::render_to` is `stream_to` over one, returning the same
    `io::Result<ExportTally>` — so there is no buffered renderer and streamed renderer to keep in
    agreement, with the `String` versions still thin wrappers for the clipboard. `render_to` is the
    wrapper the app's `Fetched` export calls: building the `OneChunk` and calling `stream_to` at the
    call site is that body verbatim, and doing so left `render_to` with no production caller at all. Two tests hold that together —
    `streaming_render_matches_the_string_render_in_every_format` for the byte-for-byte agreement
    per format, and `a_chunked_export_matches_the_same_rows_in_one_go` for the same rows split
    into blocks rendering identically; add a new format by writing the `*_chunks` and wrapping it.
    **Excel is the sixth format and the only binary one.** `export_xlsx_chunks` writes one worksheet
    — named by `sheet_name` after the source table, because Excel *rejects* a workbook whose sheet
    name breaks its rules rather than repairing it (at most 31 characters, none of `[ ] : * ? / \`,
    no leading or trailing apostrophe, and `Result` when the result is not a table's) — with a frozen
    bold header row and **typed cells**: `CellTag::Int`/`UInt`/`Float` become worksheet numbers and
    everything else a string, so the application has nothing left to guess at, which is the whole
    point of the format over a CSV that Excel then guesses at — the guess that turns `SET-1` into a
    date and drops the leading zeros off a postcode. The exception is a number a worksheet could not
    hold *exactly* — an `i64`/`u64` past 2^53, a non-finite float — which goes out as **text**,
    because a worksheet number is an `f64` and a `BIGINT` key that comes back off by one is a silent
    corruption. `rust_xlsxwriter`'s `constant_memory` feature is
    load-bearing: it spills each finished row's XML to a library-managed temp file, which is what
    holds an export to a bounded buffer at any row count — the bound `RowChunks` exists to give, and
    the one an in-memory workbook would throw away at exactly the size where it matters. That makes
    `export_xlsx_chunks` **the one function in `schemaic-core` whose tests touch the filesystem**
    (see the pure-logic invariant). Excel's own ceilings are `XLSX_MAX_ROWS` (1,048,576),
    `XLSX_MAX_COLS` (16,384) and `XLSX_MAX_CELL_CHARS` (32,767): a cell past the last is cut on a
    *character* boundary and reported through `ExportTally::cut`, while a result taller than a
    worksheet is **refused** — with an error naming CSV and JSON as the way out — rather than
    truncated, stopping at row 1,048,576 of a 3M-row table being the one loss the file would carry no
    trace of at all, and the `.part` dance means the refusal leaves the destination alone. A result
    wider than a worksheet is refused the same way: unreachable in practice, since no engine returns
    16,384 columns, but stated rather than assumed because silently dropping the columns past it
    would be the worse failure. **The
    known fidelity gap**: an empty *string* writes no cell, since `rust_xlsxwriter` emits nothing for
    one, so it reads back as NULL — the same ambiguity CSV has, deliberately not fixed here.
    And because the rendering is bytes rather than text, `ExportFormat::is_text` exists: the grid's
    **Copy** menu renders through `render()` → `String`, where `to_string`'s `unwrap_or_default`
    turns a binary rendering into the *empty* string, so an Excel entry there would silently clear
    the clipboard and report success. That menu filters on the capability while the Download menu
    still offers every format (*Data grid*) — the split `erd_export::ErdExportFormat::is_text`
    already makes for PNG — and the capability is computed from the variant rather than stored, so a
    seventh format has to answer it. `stream_to`/`render_to` carry a `W: Send` bound for this
    renderer alone: `rust_xlsxwriter` hands the writer to the thread that assembles the workbook's
    ZIP, and every sink an export already writes to (`BufWriter<File>`, `Vec<u8>`) satisfies it.
    **The formats anything reads
    back must not pass a raw-bytes cell straight through.** Such a cell is
    `model::binary_display`'s `<n bytes>` (a `Value` has no bytes variant to hold the real thing),
    and emitting that produces a file which silently stores the *placeholder* as the column's data
    on re-import. `dropped_binary_columns` finds those cells in a pre-pass — requiring a *column*
    signal **and** the cell's text to agree, since either alone is wrong in a way that loses
    data — and `withheld_binary` is the one per-cell test every withholding emitter shares, so the
    two-signals rule cannot be spelled differently in one of them. **The column signal is
    `Column::is_binary` OR `ResultSet::binary_columns`**, the second being the backend's own
    per-value assertion that the column handed over raw bytes (`ResultBuilder::mark_binary`, set from
    `ValueRef::Blob` in SQLite's `fetch_query`). It is there because on SQLite a type name cannot
    always answer: `declares_bytes` already covers every `…BLOB…` spelling, the `BINARY` family
    **and** the untyped column (`schema::sqlite_affinity("")` is `Blob`), but it cannot see a blob
    living in a column declared `TEXT`, and a column with no `origin` at all — an expression, a join,
    a CTE — has no declared type to ask. In both of those the two-signal rule degenerated to "never
    withhold" and the `<n bytes>` placeholder went into CSV, JSON and SQL as though it were the data.
    The consequence to know: in a column that has handed over raw bytes, a cell whose text reads
    exactly `<n bytes>` is now withheld — the same answer a declared `BLOB` column always gave.
    **Which emitters withhold is decided by whether anything reads the format back**, and that is
    four of the six: the SQL export (`NULL`, plus a `-- NOTE:` heading the script — a comment
    rather than a refusal, since the script still runs and the one thing it may not do is pretend
    the placeholder was the data) and **CSV, JSON and Excel**, which are exactly
    `import::ImportFormat`, including the single-column "copy this column" forms the first two have
    (a worksheet has none). Markdown and HTML keep it, deliberately:
    nothing reads those back, and there the placeholder is the *useful* rendering — blanking it
    would make a 4 MB blob indistinguishable from an empty cell and from NULL, which is less than
    what the grid itself shows. This was the SQL export alone at first, and the commit that made it
    so named CSV in its own account of the bug.
    `binary_mask` is the same answer in the shape the cell loop needs — one `bool` per column,
    indexed by `ci`. Two shapes for one fact because the `-- NOTE:` wants the ordered `Vec<usize>`
    while the loop asks per *cell*, where `Vec::contains` is a linear scan over the answer (12M of
    them on a 200k × 60 result); the same hoist `Db::convert_row` and `pg_cell` make for
    `Column::is_binary`.
    **Both are computed per chunk, and the `-- NOTE:` line is the one thing a stream genuinely
    cannot know up front.** Which columns were withheld is a fact about the rows in hand — it needs
    the column signal *and* a `binary_display` placeholder in the data to agree — and a stream never
    holds them all, so a column can carry real bytes for a million rows and meet a placeholder in
    the next block. The note is therefore emitted the first time a column is actually withheld,
    naming only the newly discovered ones and tracked in a `noted: Vec<bool>`; over a single chunk
    that collapses to exactly the old behaviour, one note before the first `INSERT` naming every
    withheld column (`the_binary_note_finds_a_column_that_only_drops_later`).
    **`ExportTally` is what a renderer returns, because a row count was never the whole result.**
    `rows`, plus three losses the file itself shows no trace of: `withheld` (those binary columns,
    named — empty for Markdown and HTML), `blanked` (`ResultSet::capped_columns`, the cells past a
    column's 512 MiB arena, which read back as the empty string) and `cut` (columns cut to
    `XLSX_MAX_CELL_CHARS`, which only the Excel renderer can produce). Nothing had ever read that
    second flag, so a streamed chunk that overran the arena wrote a file with holes in it and
    reported a full row count: the grid surfaces the arena cap with a note of its own, but a streamed
    chunk is never mounted in a grid. **`cut` is a third category rather than an arm of
    `blanked`** because the two say different things to someone deciding how much of the file to
    trust: a blanked cell is empty and looks like a NULL, a cut one *looks complete* — and it
    is the only one of the three the user can act on, by exporting that column as CSV or JSON, which
    have no cell ceiling. It is spelled `cut` and not `truncated`, the word it wanted, because
    `ResultSet::truncated` already means the *row cap* was hit and both are read in the same export
    and grid paths — `grid.rs` asks `rs.truncated` for the export-scope split a few lines from where
    it receives this tally, and two fields of one name meaning different losses is how a caveat ends
    up reporting the wrong one. `export_note` says it, `absorb` folds it and `has_caveat` counts it, on the
    rules the other two already follow. `ExportTally::note` folds one
    chunk's losses in without repeating a name, since a streamed export meets the same column in
    every chunk and a caveat that named `body` two hundred times would say less than one that names
    it once. `ExportTally::absorb` is that same question one level up — folding **another table's**
    tally in, rows summed and a column name kept once in first-seen order — because a dump writes
    many tables into one file and reports one sentence about it. It lives here rather than in the
    dump's writer loop for the reason `dump_verdict` does: written out at the call site it is a fold
    nothing can test, the caller needing a `Db`, a runtime handle and two channels to reach.
    None of the three is an error — the file is written and the rows in it are real — so the one
    thing that must not happen is the caveat going unsaid, which is `export_note(tally, name,
    streaming)`'s job: silent for a clean non-streamed save, and never silent when there is a caveat.
    `export_failure_note(message, partial)` is the same rule on the other side, and what it has to
    say changed with `part_path`: the destination is no longer opened until the export succeeds, so
    the sentence is not "your file is a fragment" but `— <name> was not changed; the rows that were
    written are in <name>.part` — the reassurance and where the partial went, in one line — while a
    `partial` of `None`, an export refused before the write started, still says nothing extra.
    `export_cancel_note(name)` is the same two facts in the cancel arm's voice, a note rather than
    an error because stopping was the user's own doing. And
    `all_rows_label(size, sorted, manual_tx)` is the Download menu's `All rows` entry, three
    disclosures made at the point of choice in place of an untested `match` in the view (*Data grid*).
  - `import.rs` — the inverse of `export.rs`: CSV/TSV, JSON (array *or* NDJSON) and Excel
    `.xlsx` → table. Format
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
    (a whole-file walk still buffers, for the key union). CSV's NULL tokens match the *trimmed*
    field, except the empty one, which is exact — a blank field is data, and `trim` is the setting
    that says otherwise. `read_sample` is bounded at `SAMPLE_MAX_BYTES` for the two streamable
    formats, because a **record count is not a byte bound**: the CSV reader sets no field-size
    limit, so one stray `"` makes the whole
    remainder of a file a single unterminated field and a sample "of 200 records" reads to EOF —
    from a file the user only meant to look at.
    **Excel is the third format, and a workbook is a file with several tables in it.**
    `infer_format` takes `xlsx`/`xlsm` and deliberately **not** `.xls`/`.xlsb` — different formats
    this reader cannot open, where guessing would trade a clear "unsupported file" for a parse error
    on a file that was never going to work. `ReadConfig::sheet` picks the worksheet (`None` = the
    first, which is what a one-sheet workbook wants), and a name that no longer matches any sheet is
    an **error rather than a fall back to the first**: a workbook edited between the preview and the
    load would otherwise import a different table than the one the user reviewed, which is the
    failure worth being loud about. `open_xlsx` opens the file and `xlsx_records` is the single
    reader the preview, the validation pass and the load all walk, as `json_records` is for JSON;
    `read_workbook_sample` is the probe's entry point, returning the preview **and** the sheet names
    from one parse, because a second call to list the sheets meant opening and inflating the whole
    workbook again on every settings change. Two properties of that walk are load-bearing and neither
    is visible in an `A1`-anchored fixture: rows are numbered from `range.start()`, since
    `worksheet_range` returns the *used* range and a sheet with a title block above its header starts
    partway down (enumerating it sent the user to look at a blank row), and a **wholly empty row is
    not a record**, since the used range is a rectangle and a blank spacer row would otherwise insert
    a row of NULLs nobody typed. `xlsx_size_refusal` is asked of the file's *stat* before any of it
    is read: CSV and JSON bound their preview at `SAMPLE_MAX_BYTES` and stop, but a ZIP's directory
    is at the end, so a workbook must be read whole before its first row can be shown — which put the
    unbounded read ahead of the memory warning meant to precede it. `cell_text` is one cell → `Field`, and its conversions are chosen
    so a value that came *out* of a database survives the trip back in: an empty cell → null; a
    number in `f64`'s shortest round-trip form, so `7.0` is `7` (an `INT` column rejects the latter,
    and Excel has no integer type to tell them apart); a date or time as ISO 8601 rather than the
    serial number Excel actually stores, since a `DATE` column otherwise receives `45292`; a bool as
    `true`/`false`, spellings `coerce` already accepts; a **duration** through `duration_hms` as
    `H:MM:SS` with hours deliberately unwrapped past 24, since a `[h]:mm:ss` cell holds a fraction of
    a day and emitting it as decimal hours put a timesheet's 8h30m into a MySQL `TIME` column as
    `8.500000` — eight and a half *seconds*, wrong by 3600× and silent, because the value coerces
    perfectly well; and a formula error in **Excel's own spelling** (`#REF!`, `#DIV/0!`, from
    calamine's `Display` rather than its `Debug`, which gives a `Div0` that appears nowhere in the
    application the user is looking at) as its own text rather than a null, so a cell the sheet
    itself could not evaluate surfaces as an `Issue` naming the row. **`ImportFormat::has_own_nulls` is the capability the null rules ask** — true for JSON
    and Excel, false for CSV — because a worksheet's empty cell is a real null and `NullRule`'s token
    list must not apply to it; it replaced two separate `match format` expressions, in `validate` and
    in `row_iter`, that had to agree with each other. `RowSourceIter::Json` is now
    `RowSourceIter::Buffered` and carries both: the same buffering for two different causes, JSON not
    knowing its columns before EOF and an `.xlsx` not being readable as a prefix at all.
    `trim_to_mapping` puts Excel on **CSV's** side rather than JSON's, the used range fixing the
    width. **`read_sample` bypasses `SAMPLE_MAX_BYTES` for a workbook** for that same prefix reason:
    a ZIP's central directory is at the end of the file, so a truncated read does not open at all and
    the bound would turn "preview a 9 MB workbook" into "this file is corrupt". What is left to
    disclose is the memory that costs: `xlsx_load_estimate` is `XLSX_MEMORY_FACTOR` (25) × the file
    size, a deliberately conservative figure and **not a measured one** — unlike `JSON_MEMORY_FACTOR`
    — because the ratio depends on how repetitive the sheet's strings are, and `XLSX_WARN_BYTES` is
    40 MiB. The modal asks **`memory_warning(format, bytes)`** and never `json_memory_warning`
    directly: one call site, so a format that needs a warning cannot be given one nothing asks for.
    **The reader is `calamine`, and an imported `.xlsx` is the first untrusted XML this app has ever
    parsed** — a ZIP of it, from wherever the user got the file. `calamine` depends on quick-xml
    **0.41**, the patched version; the 0.39 in the tree is reached only through usvg/resvg for the
    bundled SVG icons and through wayland-scanner at build time. So the duplicate `deny.toml`'s
    `[bans]` reports is load-bearing rather than untidy — deduplicating it *downwards* would put the
    untrusted parser on the vulnerable code, which is why `RUSTSEC-2026-0194`/`0195` are ignored
    there with that reachability argument written out beside them.
    `target_verdict` over `DbNodeView` is the modal's other half: whether a schema change means the
    table it is open on has *gone*. `has_table` is an
    `Option<bool>` because "I looked and it wasn't there" and "I haven't looked" are the same
    `false` to a bool and a refresh empties what there is to look at, and the caller passes
    `same_connection` because `db_nodes` holds only the **active** connection's databases — nothing
    compared them, so switching connection discarded a hand-built mapping. Pure + unit-tested.
  - `dump.rs` — the **schema + data dump**: what one replayable `.sql` file holds and in what order,
    as a `DumpPlan` of `DumpStep::Text`/`DumpStep::Rows` that `schemaic-app`'s `dump.rs` executes. It
    writes nothing and connects to nothing, so every decision in it is unit-tested.
    **The interface calls this *Export*; the code calls it a dump, and the split is deliberate.**
    `export` is already taken in this crate by `crate::export`, which renders *one result set* to a
    file — a different feature with different inputs, and a reader who meets `export` in
    `schemaic-core` should be able to assume the grid one. So the menu entries, the modal's title and
    its primary button all say Export, while `dump_run`, `DumpPlan` and `app::dump` keep the code
    name; the module doc is where the two vocabularies are reconciled, and it is what to read before
    "fixing" a `dump_` name that sits behind a menu called Export.
    **It joins two emitters that already existed and adds no third.** Structure is
    `TableInfo::create_ddl` (which routes a view on to `ddl::view_ddl`), triggers are
    `TriggerInfo::create_sql`, the closing constraints are a `ddl::ChangeSet` of
    `Change::AddForeignKey` through `ChangeSet::emit` — the emitter the apply path uses — and the
    rows are `export::ExportFormat::Sql`, streamed by the app. Identifiers go through
    `export::ident_sql`/`qualified_table`, so a dump cannot quote differently from the SQL export.
    **The row `SELECT` names its columns and is never `SELECT *`.** `exported_columns` projects
    everything `ColumnInfo::is_server_assigned` says the server does *not* fill for itself — the same
    predicate `import::insert_columns` asks, about the same columns. The renderer names every column
    the result carries, so `SELECT *` put generated columns and PostgreSQL `GENERATED ALWAYS AS
    IDENTITY` columns straight into the `INSERT` column list, which all three engines refuse (MySQL
    3105, SQLite *cannot INSERT into generated column*, PostgreSQL *cannot insert a non-DEFAULT
    value*) — the file died on its first row. PostgreSQL's identity would need an `OVERRIDING SYSTEM
    VALUE` the shared renderer has no way to emit. **The cost:** an identity column's values are not
    carried, so the restored rows are renumbered — which the **header names, column by column**,
    alongside the cycle and dropped-constraint notices. The person replaying the file is the one who
    needs to know, and it is the same silence about a `NULL`ed blob that the tally exists to break;
    a file that carries every column says nothing, because a caveat printed on every dump is one
    nobody reads (`a_column_the_file_cannot_carry_is_announced_in_it`,
    `a_file_that_carries_every_column_says_nothing_about_it`). A table whose columns are *all*
    server-assigned gets **no data step at all**: nothing about it is insertable, and `SELECT  FROM`
    would not even parse. Both halves are pinned by
    `a_generated_column_is_never_selected_into_the_insert` — which also asserts the column is still
    *declared*, since it is only the `INSERT` it has to stay out of — and
    `a_table_the_server_fills_entirely_gets_no_data_step`.
    **Foreign keys are restated after the data.** `create_ddl` deliberately emits none: for Copy DDL
    an omitted key still leaves a script that runs, which is why the ordering effort there went to
    types and views instead (`create_ddl_script`'s own account of it). A dump can't take that trade —
    a restore that silently drops every constraint is not a restore — and the trailing section is
    also what stops the insert order invalidating the file. It is **skipped for a table whose
    `create_sql` is `Some`** (`needs_fk_section`): SQLite's verbatim captured `CREATE TABLE` already
    carries the keys, and SQLite has no `ALTER TABLE … ADD CONSTRAINT` to restate them with. That
    question is asked of the *table*, never of the dialect — the data answers it directly.
    **And only a key whose target table is in the same file.** Exporting one table is a first-class
    action now — a table node's own *Export* — and `ALTER TABLE orders ADD CONSTRAINT … REFERENCES
    customers` in a file that never creates `customers` fails at restore: on PostgreSQL with no guard
    to hide behind, and *after* the rows have landed. The keys pointing outside the selection are
    dropped, counted, and **the header says how many and why**, which is the honest half of the trade
    — emitting a statement that cannot succeed was the alternative, not a completer restore. That is
    also why the whole constraints section is **decided before the header is emitted** even though it
    is written last: the header has to be able to report the count, so a future tidy that moves the
    section back into file order silently empties that sentence
    (`a_foreign_key_to_a_table_outside_the_selection_is_not_restated`).
    **The FK guard wraps the transaction, never the other way round.** `PRAGMA foreign_keys` is a
    silent no-op inside a transaction on SQLite, so a guard emitted after `BEGIN` reads correctly in
    the file and does nothing at all. `fk_guard_sql` and `transaction_sql` are the strings and `plan`
    is what composes them, which is why `the_sqlite_guard_sits_outside_the_transaction` asserts on
    the whole file rather than on either function — the bug is the composition. **PostgreSQL gets no
    guard**: `session_replication_role` is superuser-only, so offering it would be a checkbox that
    fails the restore for most roles, and the ordering plus the trailing constraints section is the
    answer there. **`target_database_sql` emits `USE` on MySQL only, and it is load-bearing.**
    `create_ddl` names a MySQL table bare — a database is not a namespace there, so there is nothing
    to qualify with — while the `INSERT`s come from the export renderer, which addresses a table
    through `qualified_table` and *does* name the database; without that line the file would create
    `orders` wherever the client is pointed and then insert into `shop`.`orders`.
    PostgreSQL needs none (both halves name the namespace) and SQLite has no qualifier at all.
    What that line is **not** is a retarget: a `mysqldump` is replayed elsewhere by editing its one
    `USE`, because its `INSERT`s name the table bare, while these name `` `shop`.`orders` `` outright
    — so the file is locked to the database it came from, and that is **known debt rather than a
    finished answer**. The fix is a way to render an unqualified target in
    `export::qualified_table`/`export_inserts_chunks`, which is the grid's export path too — too wide
    a change to make from inside this feature, so it is written down here rather than done. A second
    edge is deliberately left alone: `qualified_table(database, None, table, Postgres)` names the
    *database* as a namespace, which is wrong, but `pg::fetch_schema` always sets a
    `TableInfo::schema` so nothing here reaches it, and the fallback lives in the shared quoter where
    changing it for this one caller is the worse trade.
    **Three further statements are here because the file failed on replay, and each asks a capability
    rather than an engine.** `create_container_sql` makes the file's own container before anything
    uses it — `CREATE DATABASE IF NOT EXISTS` on MySQL, one `CREATE SCHEMA IF NOT EXISTS` per
    non-default namespace on PostgreSQL — because the primary use case is a restore onto a *fresh*
    server, where `USE shop` is ERROR 1049 and a table in `sales` is `schema "sales" does not exist`:
    the file died on line 1. `IF NOT EXISTS` throughout, since the other use case is replaying onto
    the database it came from and that must not open with an error either; PostgreSQL gets no
    `CREATE DATABASE` (it cannot be run from inside the database being restored into, and the
    connection is already pointed at one) and `public` is skipped. `drop_cascade` appends ` CASCADE`
    on PostgreSQL alone: MySQL and SQLite both have a session switch that turns key enforcement off
    for the whole load, PostgreSQL's is superuser-only and `fk_guard_sql` returns `None` there, so
    nothing else in the file protects the `DROP` — replaying a default dump onto the database it came
    from, which is how anyone tests a dump, stopped at the first parent table with *cannot drop table
    customers because other objects depend on it* and never reached the rest. Dropping the dependants
    is right precisely here, because the section below is their `CREATE` and the closing constraints
    section puts the keys back. `sequence_resync_sql` closes the PostgreSQL restore:
    `exported_columns` carries a `serial` or `GENERATED BY DEFAULT AS IDENTITY` column
    deliberately — someone
    re-importing their own keys wants them — but an *explicit* insert does not advance the sequence
    behind the column, so the restored table holds keys 1..10000 with its counter still at 1 and the
    first ordinary insert afterwards is a duplicate-key error that repeats until the counter catches
    up. It emits a `setval` per column, written so that a column with **no** sequence behind it is a
    no-op rather than an error: `pg_get_serial_sequence` answers `NULL` there, and selecting through a
    subquery with `WHERE … IS NOT NULL` means no row is produced and `setval` is never called — which
    also covers an empty table. Live-verified, and `pg_dump` emits the same for the same reason.
    MySQL's `AUTO_INCREMENT` and SQLite's `rowid` both advance from the data already in the table, so
    the gate is `ddl::supports_sequence_resync` — an exhaustive `match` beside
    `supports_column_reorder`, replacing the `dialect != SqlDialect::Postgres` this shipped as.
    **`DumpPlan::missing` is the ticked tables the dump's own fresh introspection could not find** —
    renamed, dropped or permission-revoked between the picker and the save dialog. The
    re-introspection is deliberate (a backup of a shape the server no longer has is not a backup) and
    this is its cost, so it is reported rather than silent: a file one table short of what was ticked
    looks exactly like a complete one, and only the sibling *preselect* case was ever named.
    **Ordering is a topological sort over `TableInfo::foreign_keys`** (`order_tables`): a referenced
    table before the table referencing it, views last — a view's body selects from the tables above
    it and it holds no rows to order against anything — and ties broken by name, so two dumps of one
    schema are the same file. A self-reference is not an edge, being one table that can only be
    created before itself. A real cycle is **reported, never dropped**: no creation order satisfies
    one, so an edge is broken at the smallest name, every chosen table still reaches the file, and
    `DumpPlan::cycles` is what puts the explanation in the header.
    **The views are then sorted among themselves, by the same walk.** "Views last" ordered them
    against the base tables and left them in name order against each other, and a view built on
    another view is `CREATE VIEW … FROM other_view` where `other_view` does not exist yet — MySQL
    1146, *after* the file's own `DROP VIEW IF EXISTS` has already removed it from the target. The
    dependency walk is built from `foreign_keys` and a view has none, so nothing ordered them at all.
    A view's edges are the other picked views its body names, matched as **whole words in code**
    (`intel::code_word_hits_in`) so a name inside a comment, a string literal or a longer identifier
    is not an edge. Both halves share `topo_order`, which takes its members already in name order so
    that "the first ready one" *is* the tie-break and two dumps stay byte-identical. The mask is
    built **once per definition** (`intel::code_mask`, split out of `code_word_hits` for this
    caller): the question is every picked view's name against every other's definition, V² questions
    over V definitions, and folding the lex back into the search re-lexes each definition V times. The standalone-objects section
    covers only the namespaces the chosen tables live in — a dump of `sales` has no business
    recreating `archive`'s types — and skips `ObjectItem::is_internal`, since a `serial`'s own
    sequence is created by the column's definition and restating it fails the load on a name that
    already exists. It also drops a sequence **owned by one of the chosen tables**, which
    `is_internal` cannot answer on its own: a catalogue can report the link as external while the
    column still owns the counter, the same question `DbSchema::create_ddl_script` asks and the two
    scripts must not disagree about. The owner comparison is on **`(namespace, name)`**, not the name
    — `create_ddl_script` gets away with a name because it works inside one namespace, while a
    selection spans them, so `sales.orders_id_seq` owned by the unexported `sales.orders` was being
    dropped on the strength of a chosen `public.orders`, leaving the column that defaults to it with
    nothing behind it (`a_sequence_owned_by_a_same_named_table_in_another_namespace_is_kept`). A
    sequence carries its own namespace and its owner is in that namespace, which is what makes the
    pair available to compare. It is on by default because a file without it fails on the first
    column typed as one of the database's enums. Pure + unit-tested; extend the tests that assert on the whole file
    (`file_of`) rather than on one string, since ordering across the two step kinds is where this
    module's real bugs live.
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
    arrow buttons, which must not offer one. `supports_sequence_resync` is the newest of the family
    and the only one no editor asks: does an auto-increment column draw from a **separate counter** a
    restore has to be told about? PostgreSQL's does — a `serial` or identity column is backed by a
    sequence that explicit inserts leave where it was — while MySQL's `AUTO_INCREMENT` and SQLite's
    `rowid` both advance from the data already in the table, so there is nothing to resync;
    `dump::sequence_resync_sql` is the one caller, and this replaced the `!= Postgres` it shipped as.
    `supports_database_editing` and `supports_namespace_editing` are the newest pair, for the
    **container** the rest of this module's objects live in: `CREATE`/`DROP DATABASE` on MySQL and
    PostgreSQL, and PostgreSQL's `CREATE`/`DROP SCHEMA`. They are two predicates rather than one
    because MySQL answers them *differently* — its `CREATE SCHEMA` is a synonym for
    `CREATE DATABASE`, one level up from what the change means, so a single "supports containers"
    answer would have offered `Create schema` there and quietly made a database. SQLite answers no
    to both: a database there is a file, so a create is the connection form's business and a drop
    would be deleting the user's file off disk — absent rather than dimmed, since there is nothing
    another connection could enable. **The namespace one is deliberately not called
    `supports_schema_editing`**: that name belonged to the predicate deleted two paragraphs up,
    which meant the whole of this module rather than one statement in it, and the same name for
    two opposite scopes is worse than a longer one. The menu entry is still labelled `Schema`,
    because that is what PostgreSQL calls it.
    The four changes they gate (`CreateDatabase`, `DropDatabase`, `CreateSchema`, `DropSchema`)
    are the only ones in the module that **do not address `ChangeSet::table`** — each carries its
    own name, `ddl::server_level` is the constructor that builds such a set, and
    `container_creates`/`container_drops` are the one emitter pair that reads neither `table` nor
    `qname()` (`a_container_change_ignores_the_change_sets_subject` is the pin, since an emitter
    reaching for `qname()` like every sibling would address the wrong object while the change
    list still read right). **They are two functions emitted at opposite ends of the plan, and
    the asymmetry is the point**: a container has to exist *before* anything inside it is created
    and survive *until* everything inside it is done with, so a `DROP SCHEMA` at the front would
    take the namespace away from every later statement naming `sales.orders`. Written as one
    front-loaded loop first, with a comment claiming the position was universally safe — which is
    the kind of claim a later edit builds a real bug on;
    `a_container_is_created_first_and_dropped_last` hand-builds the mixed set nothing produces
    yet. `is_server_level` splits them again along a different seam — the database pair
    runs on a connection that names no database and outside any transaction, the namespace pair on
    the ordinary in-database path — which is what `DdlScope` and `Db::run_server_ddl` are for.
    `DROP SCHEMA` is never `CASCADE`, the same call `DropObject` makes: cascading drops every
    table in the namespace, so the server is left to refuse and name what is still in there.
    **Six account changes join them in not addressing `ChangeSet::table`** — `CreateAccount`,
    `DropAccount`, `Grant`/`RevokePrivileges` and `Grant`/`RevokeRole`, the Users and privileges
    browser's write half — built by `ddl::account(subject, dialect, change)`, a sibling of
    `server_level` whose `subject` is the account's display name (`app@%`, or a bare role) and lands
    in `ChangeSet::table` only so the preview's title names what the plan is about; each change
    carries its own account. `is_account_change` groups them because every downstream question is
    the same question for all six, and the load-bearing answer is that **they are not
    `is_server_level`**. An account belongs to the server, but the server-level route deliberately
    connects to no particular database, and a PostgreSQL `GRANT SELECT ON TABLE public.users` names
    an object in *one database's* catalogue — it would grant on whatever the maintenance database
    happens to hold, or fail. So they take the ordinary in-database route, in the database the
    browser is showing privileges for, which is correct on both engines: MySQL's grant tables are
    server-wide and answer the same from any connection, and PostgreSQL's are exactly the ones the
    browser was already scoped to (`an_account_change_takes_the_ordinary_in_database_route`).
    `supports_change` answers for them in an early arm returning `users::supports_user_admin`, so it
    and the browser that offers the action cannot drift about which engines have accounts at all.
    `ChangeSet::account_statements` emits them at the **end** of the plan, called from `emit_mysql`
    and `emit_postgres` beside `container_drops` and filtered on `supports_change` like its
    neighbours — a privilege is stated *on* something, so it comes after whatever the plan creates.
    Within the group the order is **create → grant → revoke → drop**, the only one that composes: an
    account has to exist before it can be granted anything and has to still exist while it is.
    `an_account_is_created_before_it_is_granted_and_dropped_after` hand-builds the mixed set nothing
    produces yet, exactly as `a_container_is_created_first_and_dropped_last` does one level up.
    `summary`'s arms run their privilege list through `privilege_words`, which names them while
    there are three or fewer and counts them beyond: eighteen is a legal selection at MySQL's
    database level, and the summary is one line above Apply. An empty list reads **"no privileges",
    not "nothing"** — `users::privilege_sql` emits no statement for one, so the preview headed a plan
    "1 Change", listed it, and showed an empty SQL box under a dimmed Apply: a change described with
    nothing behind it. The form's Apply asks `GrantDraft::is_ready` first, so this is reachable only
    by a caller that builds the change directly. `risks` speaks for `DropAccount`,
    `RevokePrivileges` and `RevokeRole`, and the account drop's sentence says what is actually
    lost — **its privileges, not its data**, with no record of them left anywhere to put back —
    plus the surprise that anything still connected as it keeps running until it disconnects.
    `grant_change(draft, account)` is the last piece: the mapping from the grant form's Action and
    Subject dropdowns (two toggles when the test was named) to those four statements, out of the
    view because a mapping invisible in a rendered form is the
    kind that ships backwards (`the_forms_two_toggles_choose_between_exactly_four_changes`).
    `every_account_change_the_engine_accepts_emits_a_statement` is a hand-written list, extended on
    purpose when a seventh arrives, and `an_account_change_sqlite_refuses_is_withheld_not_silent` is
    the other side of it.
    What actually varies for views moved down a level, into
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
    → the same preview. One engine rule each lives here.
    `Change::RefreshView` is the exception to the draft-and-diff shape, and joins
    `TruncateTable` in coming straight off the context menu: a **materialized** view's rows
    are not part of its definition, so there is nothing to diff — the whole change is which
    of the two statements to send. `supports_concurrent_refresh` decides that from the
    view's own indexes, because `CONCURRENTLY` is a *capability of the view* rather than a
    preference: PostgreSQL takes an `ACCESS EXCLUSIVE` lock for the plain form, and refuses
    the concurrent one without a `UNIQUE` index that is neither partial nor keyed on an
    expression. Schemaic adds a fourth condition the server never states — the index must
    not be `IndexInfo::lossy`, because there what was read back is not the whole index and
    the other three were checked against a fragment. Uncertainty resolves to the plain
    refresh, which always works.
    A never-populated view is the one case the model can't see (`pg_class.relispopulated`
    isn't in it) and the server's own refusal is the answer there. **`refresh_view_change`
    is the whole decision** — is there anything to refresh, and which form does it take —
    in one function over one `TableInfo`, because the two halves used to be assembled in the
    schema menu's closure, which closes over a `Ui` and so could not be tested at all; what
    that left uncovered was never either predicate but their composition with the caller.
    `supports_change` gates
    the change to PostgreSQL exhaustively, and `view_statements` asks it again at the
    emitter: `emit_sqlite`'s `supported()` filter is the only `supports_change` in any of
    the three emitters, and this builder runs before even that one, so an engine with no
    materialized view would otherwise be handed a
    statement it has no word for. It carries no `risks`, so the preview's Apply stays
    `Primary` and no confirmation is asked; the lock is stated in `summary()` instead,
    where the SQL alone doesn't show it. MySQL's `CREATE OR REPLACE VIEW`
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
    by `drop_object` — the refusal is the capability `ObjectKind::uses_shared_changes`, which is
    what the three shared-arm constructors ask. It returns an **empty** change set for one, so a
    preview says "no changes" rather than a bare-name statement that is right up until
    the first overload; `drop_routine` takes the whole `RoutineInfo` and is the route.
    SQLite has no stored routines at all — a function there is registered by the host program,
    not stored in the database — so its arms are absent from `supports_change`,
    `supports_routine_editing` is false, and the tree grows no folder and the Create menu no entry.

    **MySQL's scheduled events** — `EventDraft` → `diff_event` → `event_alter_clauses` → the same
    preview — are the fourth browsable object kind, and the one whose shape is dictated by what
    its engine *can* do rather than what it can't: `ALTER EVENT` reaches the schedule, the status,
    the comment, the definer, the name **and** the body, so every edit here is one statement
    restating only what changed and there is no drop-and-create anywhere. A rename rides inside
    that same `ALTER` as its `RENAME TO` clause rather than being a `Change` of its own — split
    into two, the pair would need an order (rename first and the second statement must address the
    new name; alter first and a failed rename leaves a half-applied edit) for no gain, since MySQL
    gives DDL no transaction either way. The header names the event **the server holds** and
    `RENAME TO` the one the draft wants, which is why `Change::AlterEvent` carries `from` as a
    whole `EventInfo`.
    Three details are load-bearing. The clause order is **MySQL's grammar**, not a preference —
    `ON SCHEDULE`, `ON COMPLETION`, `RENAME TO`, the status, `COMMENT`, `DO`, and any other
    arrangement is a syntax error. `diff_event`'s emptiness test is `event_alter_clauses` rather
    than `draft.info == *current`, because the two sides carry session state and a `time_zone` no
    clause restates, and comparing the whole struct emitted `ALTER EVENT e` with no clauses — itself
    a syntax error — every time the lazy `SHOW CREATE` corrected one side and not the other. And
    the **definer is the one edit that lives entirely in the header**: MySQL needs at least one
    alteration clause after the name, so a definer-only change carries a restatement of
    `ON COMPLETION` with it, chosen over the status because it is provably a no-op *and* because a
    stray `ENABLE` in the preview reads as an edit somebody made.
    The one-shot warning (`Change::risks`) compares **both sides** on the alter arm: an event that
    was already `AT … NOT PRESERVE` is not made self-deleting by a comment change, and warning
    about it would make every such edit `is_destructive` and put it behind the preview's
    destructive gate. It fires where the plan *introduces* the combination — the way every other
    comparison in this module works.
    A `CREATE`/`ALTER` that restates the body takes **no trailing semicolon** — the body is a
    compound whose own statements end in `;` — and a `DROP` does; `event_restates_body` is asked
    once and answers both the `DO` clause and the terminator. `supports_event_editing` is the
    capability every surface asks (`supports_change` answers the three event arms with an
    exhaustive dialect match ahead of its blanket "everything but SQLite"), and
    `uses_shared_changes` is false for `ObjectKind::Event` for its own reason: the comment is a
    clause of `ALTER EVENT`, so `SetObjectComment` would emit a `COMMENT ON EVENT` no engine has.
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
    `fit_toolbar` is the diagram **toolbar's responsive rule**, and it is here rather than in the
    view because it is arithmetic over widths: given the room available and what each optional
    group costs, it drops them in a fixed priority order — the count pills, then the zoom stepper,
    then the export button — and returns a `ToolbarFit`. The order is least-useful-first: the pills
    restate what the diagram already shows, the zoom stepper duplicates Ctrl+wheel and its
    percentage is only a readout, and export is the one of the three with no other way to reach it.
    The scope breadcrumb and the Fit/Reset pair never drop — those two are the *recovery* controls,
    and a diagram panned off screen is unusable without them. Each width the caller passes is what
    the toolbar **loses by hiding that group**, the flex gap included, since taffy drops a
    `Display::None` child before the line the gaps are distributed over is built. An unmeasured
    toolbar (0) shows everything, so the first frame is one of overflow rather than a stripped bar
    that fills in. Four properties are pinned: the drop order, that the result is always a *prefix*
    of it (never a hole), that widening never hides a group (a non-monotone fit would flicker one
    in and out under a window drag), and the unmeasured arm.
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
    dropping — a cap the app used to keep private, moved here once the log became exportable, since
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
  - `users.rs` — the **Users and privileges** browser's model: which accounts a server knows, and
    what each of them is allowed to do, over an already-fetched snapshot. No DB, no UI, no engine,
    and deliberately the same shape as `activity.rs` — the fetch is one query set per engine in
    `schemaic-db`, because the catalogues share nothing, while every decision about what a
    `Principal` *says* lives here with the tests, because a principal means the same thing whichever
    engine produced it. `supports_users` is the capability (**false for SQLite, and that is a
    statement about SQLite rather than unfinished work**: it is a library linked into this process
    and its access control is the filesystem's — the database file's permissions, granted to an OS
    user by the OS — so there is no account to browse and no statement that would create one), and
    `supports_user_admin` is *computed* from it rather than spelling out a second `!= Sqlite`. A
    `Principal` is `name`/`host`/`kind`/`system`/`attributes`, and the `Option` on `host` is the one
    place the two engines disagree about what an account *is*: on MySQL/MariaDB it **is** the
    `(user, host)` pair, where `'app'@'%'` and `'app'@'localhost'` are two accounts with different
    passwords and different privileges, while a PostgreSQL role is not host-scoped at all — its host
    rules live in `pg_hba.conf`, a file, and no catalogue publishes it. **A *role* carries no host on
    any engine here, and `from_mysql_rows` drops the one the catalogue stored rather than working
    around it in each statement builder.** MariaDB keeps `''` for a role and MySQL 8 keeps `'%'`, and
    the first reading of that was that a statement naming one still has to match what the server
    stored; measured on MariaDB 10.11, the server says the opposite. `SHOW GRANTS FOR 'r'@''` is
    ERROR 1141, `GRANT SELECT ON world.* TO 'r'@''` is ERROR 1133 (*Can't find any matching row in
    the user table*), and `DROP ROLE 'r'@''` is ERROR 1064 — a **syntax** error, because MariaDB's
    `DROP ROLE` grammar has no `@host` in it at all — while the bare name works for all three, and on
    MySQL 8 a bare role name resolves to the `%` row it stored. That was three of the browser's four
    role actions broken on MariaDB. It is fixed in the fold and not in the builders because the
    *display* was wrong by the same value: `Some("")` renders through `display()` as `readers@`,
    trailing `@`, in the list, the detail heading, the preview's subject and the Drop confirm's
    title. `a_role_carries_no_host_because_no_statement_naming_one_accepts_it` pins it, and its
    counterpart `a_user_keeps_the_host_that_makes_it_a_distinct_account` pins the half that must not
    move with it. `system` marks the accounts the server owns and maintains (the reserved
    `mysql.`/`mariadb.` prefixes, PostgreSQL's `pg_` predefined roles, **and the one exact name
    `mysql`**), and they are **kept rather than filtered** — "why can `pg_monitor` read that"
    is a real question — but `sort_principals` leads its key with the flag, because PostgreSQL 16
    ships fourteen `pg_*` roles and a fresh cluster has two accounts of its own, so sorting by name
    alone buries the ones an administrator actually made in a list that is four-fifths furniture.
    **`mysql` is on that list as a name rather than as a prefix because the prefix cannot reach it** —
    `"mysql"` does not start with `"mysql."`. MariaDB's `mysql_install_db` creates `mysql@localhost`
    for unix-socket authentication of the OS `mysql` user, and while it was uncovered that account
    sorted among the administrator's own, undimmed, and was offered Privileges and Drop — the two
    actions the browser withholds from server-owned accounts precisely because changing them breaks
    the server. The trade is named where the match is: an administrator who named a real account
    `mysql` finds it read-only here, against a one-click `DROP USER` on the server's own login.
    `an_account_that_only_looks_like_the_servers_is_not_marked_as_it` keeps `mysqldump` and
    `mariadbctl` on the administrator's side of that line.
    **`MyUserRow`'s every field but the pair is an `Option`, because the two servers disagree about
    which columns exist**: MariaDB 10.11 has `is_role` and no `account_locked`, MySQL 8.4 the
    reverse, and selecting the union fails outright with `Unknown column`. So `None` here means
    *this server does not publish it*, never *no*, and an absent column produces **no attribute at
    all** rather than a `No` the browser would be asserting on the server's behalf
    (`a_column_the_server_lacks_produces_no_attribute`). **Role detection is MariaDB's `is_role`
    flag or nothing**: MySQL 8 has no such column and implements `CREATE ROLE` as a locked,
    password-expired user, so reading that pair back as "role" would relabel every genuinely locked
    account as one — on MySQL every row is a `User` and the Locked attribute says the rest, which
    `a_locked_mysql_account_is_not_guessed_to_be_a_role` pins. On PostgreSQL `rolcanlogin` is the
    user/role split, and it is the only split PostgreSQL makes; `from_pg_rows` lists only the
    attributes that are *set*, since a role with nine "No" rows is where `Superuser` stops standing
    out. `account_sql` is the account as **executed** SQL names it (`SHOW GRANTS FOR 'app'@'%'`),
    and it is deliberately **not** a fifth identifier quoter: MySQL spells an account as two *string
    literals*, so it goes through the one literal quoter (`schema::ddl_string` → `export::sql_literal`,
    which already knows MySQL escapes a backslash inside a literal and PostgreSQL does not), and
    the host-less half is `export::ident_sql` — PostgreSQL's roles, and a MySQL/MariaDB role too now
    that one carries no host, which is the branch every statement naming a role takes.
    `parse_grantee` is the inverse, for the fallback fetch: it splits
    `information_schema.USER_PRIVILEGES`'s `'app'@'%'` cell back into the pair by **unquoting through
    `sql::skip_noncode`** rather than splitting on `@`, because
    `'a@b'@'%'` is a legal account and the first `@` would invent two — and it unquotes **by the rule
    the scanner scanned with**. `skip_noncode` reads the literal as MySQL, where a backslash escapes,
    so undoing `''` alone left `'o\'brien'@'%'` scanned as one literal and unquoted to `o\'brien`:
    the backslash still in the account name, and every statement then built against an account that
    does not exist. `unquote_mysql_literal` is that narrow inverse — the quote and backslash forms
    and not the rest of MySQL's escape table (the newline, NUL and Ctrl-Z spellings), because a
    half-known table is worse than a stated one. Latent rather than live, since
    `information_schema` writes the doubled form; what was wrong was the two rules disagreeing
    (`a_backslash_escape_is_undone_the_way_the_scanner_read_it`).
    **PostgreSQL has no `SHOW GRANTS`, so `pg_grant_statements` has to write what MySQL merely
    repeats.** `PgAclRow` is one `aclexplode` row joined back to its object, and the statements are
    grouped by object **and** by `grantable`, because `WITH GRANT OPTION` belongs to the statement
    rather than to the privilege and folding two privileges that disagree about it into one line
    would claim it for both. The grouping is **one pass, comparing each row against the group before
    it** rather than searching every group so far: all four of `pg::fetch_grants`'s queries order by
    object, so a row either continues the group before it or starts a new one and the search back is
    work with no answer to find. The old shape also cloned two `String`s per row for a key it usually
    threw away — a role with privileges on every table of a 500-table schema is 3,500 rows and ~500
    groups, roughly 875,000 String-pair comparisons. A row arriving out of order **opens a second
    group naming the same object** rather than corrupting anything, so the ordering is an
    optimisation the caller supplies and not a contract it can break; the two halves are
    `rows_in_object_order_become_one_statement_per_object` and
    `rows_out_of_object_order_still_name_the_right_privileges`. A complete set collapses to
    `ALL PRIVILEGES` — what the administrator almost certainly typed, and what fits on a line where
    seven comma-separated words do not — while a privilege the kind's list doesn't know (a newer
    server's) sorts **last**, is still printed, and blocks the collapse. `PgObjectKind` covers databases, schemas, tables and sequences only:
    functions and procedures are absent on purpose, since `GRANT EXECUTE ON FUNCTION` must name the
    argument types and an overloaded name alone is ambiguous, so the `Grants::note` saying so beats a
    statement naming the wrong overload. The grantee is taken **unquoted** and quoted here with
    `export::ident_if_needed` (`pg_ident`) — SQL the user only reads — which is exactly what keeps it
    from being confused with `account_sql`'s unconditional quoting for SQL that runs.
    `Grants { statements, note }` is the honesty half, and `pg_scope_note` writes the note:
    PostgreSQL keeps schema, table and sequence privileges in the catalogue of the database holding
    the object, so one connection answers for one database and the note names which. A privilege
    screen that is quietly partial is the one way this feature can mislead.
    **`redact_secrets` is why nothing here can put a credential on screen.** MariaDB's `SHOW GRANTS`
    carries the account's stored hash inline (`IDENTIFIED BY PASSWORD '*01E8…'`), and for
    `mysql_native_password` that hash *is* the credential — the client proves knowledge of it, so
    anyone reading it off a screenshot can authenticate as the account. **The rule is positional,
    not a list of spellings**: every single-quoted literal after an `IDENTIFIED` keyword is replaced
    with `<hidden>` — single-quoted only, because MySQL also reads `"…"` as a string while on
    PostgreSQL the same bytes are an identifier, and blanking those would blank out an object name —
    *except* one introduced by `WITH` or `VIA`, which names the plugin rather than the
    secret, and scanning stops at `REQUIRE`, whose literals are X.509 subjects and issuers — public
    by nature, and the reason a blanket "redact every literal" would be worse rather than safer.
    That covers `IDENTIFIED BY 'plaintext'`, MySQL's `IDENTIFIED WITH 'plugin' AS '$A$005$…'` and
    MariaDB's `IDENTIFIED VIA … USING '*01E8…'` without knowing which server wrote which. Literal
    boundaries come from `sql::skip_noncode` — the one boundary lexer — so an escaped quote inside a
    hash cannot end the span early and leave its tail on screen
    (`an_escaped_quote_inside_the_secret_does_not_end_it_early`).
    **The write half's statement builders live here beside the readers**, for the reason everything
    else in this module does: what a `GRANT` says is decided the same way whichever engine will run
    it, and here it has tests. `GrantLevel` is one enum with per-dialect arms — `Global`,
    `Database`, `Schema`, `Table { qualifier, name }`, `Sequence { qualifier, name }` — because
    **the two engines mean different things by the same word**: a MySQL `GRANT … ON db.*` reaches
    every table in the database, while a PostgreSQL `GRANT … ON DATABASE d` grants on the database
    *object* (`CONNECT`/`CREATE`/`TEMPORARY`) and says nothing about the tables in it, so one shared
    "database level" would change meaning under the user on a connection switch. `levels_for` says
    which arms an engine offers: MySQL has `Global` and no `Schema`/`Sequence`, and **PostgreSQL has
    no `Global` at all** — a statement about PostgreSQL rather than a gap, since its cluster-wide
    powers are role *attributes* (`SUPERUSER`, `CREATEDB`, `REPLICATION`) carried on the role and set
    with `ALTER ROLE`, not privileges `GRANT` can express. SQLite gets an empty list rather than a
    panic, `supports_user_admin` being the gate that should have stopped the caller.
    `privileges_for(dialect, level)` is **curated, not exhaustive, on MySQL's global level**:
    `GRANT` there also takes the server-administration privileges (`SHUTDOWN`, `SUPER`, and MySQL
    8's few dozen dynamic ones), a list that differs by server *and by version*, that no catalogue
    publishes as a menu, and that nobody should be handed a checkbox for next to `SELECT` — the
    editor is where the whole language is available. PostgreSQL's lists are the complete ones and
    are *read off* `PgObjectKind::all_privileges` rather than restated, so the set this form offers
    and the set that reads back as `ALL PRIVILEGES` cannot disagree;
    `granting_every_postgres_table_privilege_reads_back_as_all_privileges` pins exactly that
    composition by ticking every box and running the result through `pg_grant_statements`.
    `PrivilegeChange` + `privilege_sql(change, dialect, revoke)` write the `GRANT`/`REVOKE` — one
    struct for both directions, since what a revoke takes away is exactly what a grant gives, and
    `WITH GRANT OPTION` is ignored on the revoke side rather than given a second field nobody sets.
    It returns `None` for an empty privilege list, because `GRANT ON db.*` is a syntax error: the
    backstop *under* the form's own Apply gate, not the gate. `RoleChange` + `role_sql` are the
    membership pair, and `AccountDraft` + `account_draft_sql` the `CREATE USER`/`CREATE ROLE` — a
    role takes no host and no password on either engine, and an empty password emits **no clause at
    all**, which is a real account on both (PostgreSQL's must authenticate some other way, MySQL's
    has simply not been given one yet). `drop_account_sql` is the last, and like `DropDatabase` never
    `IF EXISTS`: the account came off the browser's list, so one that isn't there means the list is
    stale and a drop that dropped nothing is about to be reported as a success.
    `GrantDraft` is the grant form's state, held here so the view holds none of it — `subject`
    (`Privileges` or `Role`), `revoke`, `level`, `qualifier`, `name`, `privileges`,
    `with_grant_option`, `role`, `with_admin_option`. **Two name fields for five levels**, because
    that is how the levels nest: a database and a schema each name one thing, a table and a sequence
    name one thing *inside* another, and the whole-server level names none. `level()`, `change()` and
    `role_change()` answer what it describes, and `is_ready` is the **single completeness question
    Apply reads**, so a form that cannot produce a statement cannot offer one — the alternative is an
    enabled button whose plan turns out to be `GRANT ON …`, refused a second time much further from
    the user. `toggle` keeps the ticked privileges in the engine's own documented order however the
    boxes were clicked, so two people who pick the same set get the same statement.
    **Object names in these statements go through `export::ident_sql`** and its unconditional
    quoting, because this is SQL that is *executed* — the opposite call to the browser's read-only
    `GRANT` list and its `ident_if_needed` (`pg_ident`), which is the same distinction one paragraph
    up. Accounts themselves still go through `account_sql`.
  - `diff.rs` — the line-level diff behind the inline-AI (Ctrl+K) preview. `line_diff` is the LCS
    pass, one tagged row per displayed line (context / removed / added); `inline_plan` re-addresses
    those rows as **document line numbers**, which is what lets the UI draw the suggestion in the
    editor's own line flow instead of in a box over it — the renderer needs to know which of the
    user's lines to fade and where to hang the new ones, not how to lay out a list. Consecutive
    non-`Equal` rows group into one `InlineHunk`; its `del` is the half-open range of document lines
    the change removes, and its additions hang off the **last** line it removes, which is what puts
    the `+` rows below the `−` rows they replace. A pure insertion falls back to the line above it,
    and `before` — the one case the renderer special-cases — is true only for an insertion at the
    very top of the buffer, which has no preceding line to anchor to. `line_span` is the companion
    for the state *before* there is a plan: the request is captured as a byte range, everything
    rendering against the editor surface is keyed on line numbers, and the lines being *worked on*
    have to be named the same way the lines being replaced are. Its edge is pinned by
    `a_range_ending_on_a_newline_does_not_reach_the_next_line` — the last line is the one holding
    the range's **final byte**, not the one holding `end`, so a range stopping just after a newline
    covers the line it ended and not the empty one after it. `DiffRow`/`build_diff_rows`/
    `DIFF_CONTEXT`, which built the old preview *list*, went with the box that displayed it.
    **`inline_splice` is one function because the preview and the accept have to agree.** It returns
    both the text that will actually go into `full[start..end]` and the whole buffer that results, so
    `inline_plan` diffs the same string Accept writes. Computed separately, a CRLF buffer plus an LF
    reply — the ordinary case on Windows, where the clipboard hands you CRLF and neither
    `clipboard-win` nor floem's editor normalises it — made the plan come back *empty*, because
    `str::lines` strips a trailing `\r`: the footer read "No changes suggested" while Accept, gated
    on the state rather than on the plan, converted every line ending in the range. So the reply is
    brought into the buffer's own ending first, taken from whatever its **first** newline uses; a
    buffer with no newline is left alone, there being nothing to disagree with.
  - `snippet.rs` — the snippet library: named saved queries, persisted to `snippets.json`.
    `applies` answers whether a snippet may be offered on a connection, `grouped` builds the
    panel's headings (**narrowest bucket first** — this connection, this engine, everywhere — each
    snippet under exactly one, empty buckets omitted), `matches_query` is the panel's filter
    (name/abbrev/body, the body whitespace-collapsed the way `history::matches_query` reads a
    statement), `by_abbrev` is the completion trigger (whole-word, case-insensitive, **a snippet
    the user wrote wins, then the narrowest scope**), `scope_options` is the scope picker's three
    choices **in the bands' own order** — a test pins each choice to the heading a row moves to
    when it is picked, so the picker and `grouped` cannot drift — plus
    `next_id`/`touch`/`remove`/`clear_conn`.
    **A pack of built-in snippets ships in the code**, not in anybody's `snippets.json`:
    `builtins(dialect)` returns them per engine (`Source::Builtin`, `Scope::Dialect`), and
    `library(user, dialect)` is the merged view — user snippets first, then the pack — that every
    consumer must be handed, because `applies`/`grouped`/`by_abbrev` all answer about a list. They
    are kept in code so a later release can fix one; a shipped snippet is not editable or
    deletable (the panel offers Duplicate instead), which is exactly why the *source* has to be
    `by_abbrev`'s first ranking key: every built-in is `Scope::Dialect`, so ranking scope first put
    the pack above a user snippet moved to "All connections" — a snippet they wrote, losing its
    abbrev to one they cannot delete or re-spell.
    `clear_conn`/`count_conn` drop a deleted connection's `Scope::Conn` snippets, for the two
    reasons `delete_conn_now` states about its eleven siblings: a deleted connection should not be
    reconstructable from what is left on disk, and connection ids are **recycled**, so a snippet
    left behind is inherited by the next connection to take the freed id, under a heading reading
    "THIS CONNECTION".
    **Scope is `Global | Dialect | Conn`, not a `conn_id` like `history.rs` uses** — that is the
    difference between a library and a log: a "running queries" snippet is wanted on every MySQL
    connection, not on the one it was saved from. Nothing is capped or evicted, for the same
    reason. A body's `:name` placeholders are not stored (they would go stale on the first edit)
    and need no tab-stop syntax of their own — inserting the body hands them to `params`.
    `Scope` is a **preserving** persisted enum: `Unknown(String)` keeps the text it didn't
    recognise and the hand-written `Serialize` writes it back verbatim, the rule
    `search_history::ObjectTag` states, because this file is rewritten whole on every change.
    `next_id` is max-plus-one **over the user's snippets only** — it filters out anything at or
    above `BUILTIN_ID_BASE`, so the pack's ids never raise the counter and a careless caller
    passing the merged `library()` list gets the same answer as one passing the user's. It
    therefore **reuses a deleted snippet's id**; that is safe only while no id outlives the file,
    which is why snippet activations are not recorded in `search_history.json` — a test pins both
    limits.
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
    one the user cancels *was* dispatched and may have written something. `remove` is the row
    menu's single-entry delete, and its key is **`(conn_id, sql)` because that is the identity
    `push` maintains**: `push` drops any earlier entry with the same pair before inserting, so at
    most one can be in the log at a time, and `remove` is that predicate read backwards. The
    coupling is real and neither function states it alone, which is why the test that holds the
    seam is `remove_undoes_a_push_and_a_push_undoes_a_remove` rather than a pin on either side of
    it. `run_id` looks like the better key and is not — it is `0` on everything written before it
    existed, so it names a *run* and not an entry, and deleting by it would take every legacy row
    at once; `ts` is no better, since re-running a statement bubbles its entry to the top with a
    fresh one and a menu built before that bump would miss its target. It `retain`s rather than
    dropping the first match (`push` cannot make a duplicate, a hand-edited `history.json` can, and
    leaving a copy behind reads as the delete having failed) and returns whether anything went, for
    `push`'s reason: the app rewrites the whole file on every mutation, so a right-click on a row
    that is already gone should not spend that write. `RunResult::loaded`/
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
    (Windows/Linux/macOS): `draws_own_controls`, `own_control_count`, `draws_own_resize_border`,
    `wants_drop_shadow`, `leading_inset`. **Ask the capability, never `cfg!(target_os = …)` at the
    use site** — the same
    rule the engines follow, for the same reason. The split is not cosmetic: floem reads that one
    flag as *undecorated* on Windows/Linux but as a *transparent* title bar over a full-size content
    view on macOS, so the traffic lights, the native resize border and the move behaviour all
    survive there. What macOS costs us instead is space — the lights are drawn over our header, so
    `leading_inset` reserves it. The Windows half is the one with teeth: winit strips
    `WS_CAPTION | WS_SIZEBOX` from an undecorated window, so without our own edge zones the window
    cannot be resized at all. `ui::window_chrome` draws what this module decides.
    `own_control_count` is the *how much* to `draws_own_controls`'s *whether*, and it has a caller
    of its own: the band the app lays over a modal's backdrop to keep the title bar working has to
    stop exactly where the caption buttons begin. A count, not a width — the pixels are the UI's.
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
    never happened. **For a statement that *failed*, the list is necessary but not sufficient**, and
    `failure_committed(engine, sql, tx_alive)` is the pair: MySQL's implicit commit sits between the
    parser and the executor, so a parsed-but-rejected `DROP TABLE nosuch` (`ERROR 1051`) has
    committed while a syntax error over the same leading keyword (`ALTER TABLE t GARBAGE`,
    `ERROR 1064`) has not — verified against MariaDB 10.11.14, where `@@in_transaction` reads `0`
    and `1` respectively. Only the server can tell those apart, so the text is consulted first (the
    round trip is paid only where the answer could matter) and `tx_alive` decides;
    `None` — not asked, or the server could not answer — keeps the conservative reading that was
    there before the probe, rather than trading a silent no-op Rollback for its mirror, a pill
    saying *Idle* over a transaction whose next statement's `BEGIN` would commit it.
    `StmtOutcome::FailedAndCommitted` is the confirmed case and the only outcome `failed_message`
    appends its disclosure to, because that loss is otherwise invisible: the statements folded in are
    already permanent, **Rollback** succeeds and undoes nothing, and the pill going quiet is the
    only thing on screen that moved. `StmtOutcome::NotSent` is the opposite error — a run cancelled
    (by the user or by the statement timeout) while still *queued* behind the tab's own connection,
    which never reached the server at all. It is reported apart from `Cancelled`, which now means
    strictly *dispatched and killed*, because the two have opposite consequences on PostgreSQL:
    folding the second as the first put **Tx aborted — rollback to continue** over a healthy
    transaction and offered discarding work as the only way on. `timeout_reached` is the same
    distinction for the watchdog, which is armed around the whole run — connect, `BEGIN`, statement,
    rows — deliberately, so its flag answers "did the clock run out" and not "what ran too long";
    `not_sent_message` is what such a run says instead, and its load-bearing half is that the
    transaction is untouched. `StmtOutcome::FailedIsolated` is what a `SAVEPOINT`-wrapped grid
    write reports, and it must not poison PostgreSQL, or one bad cell edit would tell the user their whole
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
  - `typename.rs` — **taking a declared type apart**, and the only place that knows where a type
    string's parentheses are. `split` → `TypeText{head, args, tail}` (all borrowed and trimmed; the
    closing paren is the **last** one, because an `ENUM` member may contain one), `base` →
    `int(11) unsigned` → `int unsigned` (the parenthesised part dropped, the words after it kept,
    which is what puts `timestamp(3) with time zone` on the same match arm as its unparameterised
    spelling), `args`, `leading_word` and `is_unsigned`. Three parts of the app read a declared type
    and each had its own splitter: `ddl::split_type` (are these two types the same),
    `celledit::base_type`/`type_args` (which editor does this column get) and `import::classify`
    (what kind of value goes in it) — the first two line-for-line copies that had already drifted on
    an unclosed paren, the third splitting on `['(', ' ']` and taking the first word, which is a
    fourth answer rather than a simplification. The *questions* stay different — `leading_word`
    exists because `classify` matches a keyword list and wants `timestamp` where `ddl` wants
    `timestamp without time zone` — but where the parentheses are is answered once.
    `is_unsigned` asks the **base**, not the raw text: a `contains("unsigned")` called
    `enum('unsigned','signed')` an unsigned column, harmless only because the flag is read on the
    integer arm alone. Nothing here understands what is *inside* the parens — that is
    `celledit::value_list`, which goes through `sql::skip_noncode` because a member may contain a
    quote, a comma or a `)`.
  - `format.rs` — per-column display formatters (`ColumnFormat`/`apply`: epoch→datetime, bytes,
    bool). Display-only; edit/copy stay raw. Persisted to `format.json`.
  - `conn_import.rs` — reading connections **out of other tools**: a pasted URL/DSN, DBeaver's
    `data-sources.json`, DataGrip's (or any JetBrains IDE's) `dataSources.xml`, and the three
    plain-text files the command-line clients read — `~/.my.cnf`, `~/.pgpass`,
    `~/.pg_service.conf`. Pure over *text*: `conn_sources` in the app finds and reads the files,
    this decides what they mean, which is what makes the whole surface unit-testable. Every parser
    answers with `Imported` values carrying **`id: 0`** and a *suggested* name — an import is a
    proposal, and the app assigns the real id and uniquifies the name only when the user accepts a
    row. Nothing here touches the keyring.
    `scan` is the one entry point, and its **order is the design**: `.pgpass` passwords are applied
    (`fill_missing_passwords`) after every source has parsed, so a `*:*:*:me:secret` line can
    complete the twelve server-only rows DataGrip just handed over; `dedupe` collapses repeats
    *after* that, keeping the **first** description of a server, so the survivor is the one that
    already has both the name a human chose and the password; and `mark_existing` runs last, so a
    row that repeats a saved connection is shown with `ImportNote::AlreadySaved` and left unticked
    rather than hidden — a stale saved copy is exactly the case where the import is the one worth
    keeping. `same_endpoint` is that identity, and deliberately *not* the app's `conn_id`, which a
    row that has never been saved cannot have: engine + host + port + database + user, or the file
    path on SQLite (separators normalised, since a JDBC URL writes `/` on a platform whose paths
    use `\`). The **user is part of it** — two logins on one server are two connections.
    `merge_rows`/`merge_skipped` fold one source's result into a list already on screen, and they
    live here rather than at the three call sites because a paste, a picked file and a scan all add
    to the same list: the moment they disagree about ticking or about duplicates, the user gets a
    different answer depending on which button they pressed. `merge_rows` **appends, never
    replaces** — the UI keys its selection by index into that list, so growth at the end is the
    whole of what makes those indices safe — and answers a repeat with the position already holding
    it, ticking through `Imported::preselected` so a row duplicating a *saved* connection arrives
    unticked whatever produced it. `merge_skipped` is the other half, and exists because it was
    missing: without it a second scan turned three Oracle data sources into "6 entries were not
    imported", naming each one twice. `fill_missing_passwords` **retracts
    `ImportNote::NoPassword`** when it fills — the order inside `scan` would do on its own, but a
    caller completing an already-scanned row (a hand-picked file, whose passwords arrive
    afterwards) would otherwise show "No password in the source" on a row that has one.
    `parse_url` accepts more than a strict URL parser would, because the strings people hold are
    not strict URLs: a `DATABASE_URL=…` line lifted out of a `.env` (with `export`, with quotes), a
    `jdbc:` wrapper, SQLAlchemy's `postgresql+psycopg2`, libpq's comma-separated host list, a
    bracketed IPv6 literal, and the `?user=&password=&sslmode=` parameters JDBC carries instead of
    a userinfo. A scheme it doesn't know is `UrlError::UnknownScheme`, *except* when only digits
    follow it — that is `localhost:3306`, and calling its host an unknown engine sends the reader
    looking in the wrong place.
    A driver this app has no engine for is `Skipped` **by name** rather than bent onto the nearest
    engine: a MySQL connection silently pointed at an Oracle server is a worse answer than an
    honest omission, and the modal says how many were left behind. DBeaver names its engine twice
    and only the *driver* distinguishes MariaDB (it ships under the `mysql` provider); its rows are
    sorted by name because the file is a JSON object keyed by internal ids, so "the order in the
    file" is not an order anyone chose. The DataGrip reader is a narrow element scan
    (`<data-source>`'s `name`, `<jdbc-url>`, `<user-name>`, `<driver-ref>`) rather than a real XML
    parse — `schemaic-core` has no XML dependency, and if JetBrains ever moves that shape this
    reports zero connections rather than wrong ones. `.my.cnf`'s `[client]` is the base every
    client group inherits, so a `[client_prod]` is read *layered on it* (what
    `--defaults-group-suffix` does); `[mysqld]` is the server's own configuration and is not a
    client at all.
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
    **`Tls` and `SslMode` are the transport half**, and they live here rather than in
    `schemaic-db` for the reason the module exists: this is the saved connection, and the mode is a
    saved field. Five rungs — `Disable` / `Prefer` / `Require` / `VerifyCa` / `VerifyFull` — asked
    through four predicates (`negotiates_tls`, `requires_tls`, `verifies_certificate`,
    `verifies_hostname`) rather than matched at a call site, plus `stronger_of`, which is the rule
    for merging two descriptions of one connection and the direction an *import* must move
    (`conn_import` had a DBeaver SSL block silently clamping a URL's `verify-full` down to
    `require`). `Tls::plan` collapses the five into the four decisions a handshake is made of —
    `TlsPlan` — which is what `schemaic_db::tls` translates for the two drivers; `Prefer` alone
    carries `fallback_to_plaintext`, and `hostname_override` is what keeps a **tunnelled**
    `verify-full` checking the far end's certificate instead of `127.0.0.1`. The `SslModeRaw` shim
    is the one `#[serde(other)]` on this type that does **not** degrade to `default()`: an unknown
    mode resolves to `STRICTEST`, because the default is `Disable` and a Velopack rollback reading a
    newer build's file would otherwise turn an encrypted connection into a plaintext one, silently,
    and then persist that on the next save. `caveat` is the one place a rung's words and its effect
    are allowed to differ per engine, and it carries exactly one sentence today — see
    `db/tls.rs` for the driver defect behind it. **The connection's `database`** is the other field
    the same range added: a *default*, never an override, resolved by `Connection::default_database`
    and honoured by `schema::first_bindable` (which is what makes a tab open where the form says it
    will) — and deliberately not the same thing as "no database at all", which `schemaic_db`'s
    `Scope::Server` spells separately.
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
    `EventInfo` is the **MySQL scheduled event**, and it is the one object modelled for a single
    engine's sake: PostgreSQL's nearest equivalent is `pg_cron`, an extension with its own
    catalogue and no `CREATE EVENT` grammar, and SQLite has no scheduler at all. Three things
    about the model are decisions rather than transcription. **`EventSchedule` is two shapes, not
    one nullable set of four fields** — `AT` takes a timestamp and nothing else, `EVERY` takes an
    interval and optional bounds, and a single struct would have let a draft describe a syntax
    error the form then had to refuse separately. **Every timestamp and interval quantity is held
    as SQL, not as a value**: the catalogue reports `2026-01-01 03:00:00` and the reader
    (`event_time_expr`/`event_interval_expr`) is what quotes it, once, because a field that can
    only hold a literal cannot express `CURRENT_TIMESTAMP + INTERVAL 1 HOUR` — the same contract a
    column default already has, and the same reason `event_interval_expr` tests the *value* rather
    than the unit, so a compound unit this build has never heard of is still quoted. And
    **`time_zone` is modelled** even though `CREATE EVENT` has no clause for it: the schedule is
    read in the session's zone, the server records which one, and an edit applied from another
    zone moves every future firing — so it rides in the session wrapper beside `sql_mode`
    (`ddl::event_session_wrapped`). `EventStatus::SlavesideDisabled` is a third state rather than
    a flavour of `Disabled` because a replica sets it for itself and restating the wrong one is a
    real edit; `EventSource` is the lazy `SHOW CREATE EVENT` read, and exists for exactly the
    reason `RoutineSource` does, with the consequence one notch milder — `ALTER EVENT` edits in
    place, so a body restated from the escape-resolved copy is *refused* rather than lost.

    **The standalone objects** — `EnumInfo`/`DomainInfo`/`SequenceInfo`, PostgreSQL's, plus the
    routines above and the events beside them — sit here beside the tables **on `DbSchema` itself**
    rather than being
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
    unavailable) and `forget` (delete). `RETIRED_SECRET_SUFFIXES` is the fourth transform and the
    least obvious: a secret this app **stopped writing** still has to be cleaned off the machines
    that stored it, so every save and every delete sweeps it. Its one member is the TLS client-key
    passphrase, collected and stored by an earlier build for a feature that could never work —
    `TlsPlan` carried no passphrase, so an encrypted key always failed blaming the file — and
    removing the `SecretKind` arm alone would have stranded that entry in the keyring, unread and
    undeletable through the app. The real keyring-backed store lives in `schemaic-app`'s
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
  - `celledit.rs` — **which editor a column's cells get**, and the value rules that editor
    enforces. `editor_for_type` reads a *declared* type into a `CellEditor` (`Bool(BoolWire)`,
    `Enum(members)`, `Set(members)`, `Date`, `DateTime(Zoned)`, else `Text`) and `editor_for_column` adds
    the one thing the type text can't answer — a PostgreSQL enum type's members, which live in
    `DbSchema::enums`. **Declared, never the wire type**: MySQL sends an `ENUM` as
    `MYSQL_TYPE_STRING` and a `BOOLEAN` as `TINYINT`, so `Column::type_name` knows neither the
    member list nor the `tinyint(1)` width, and a result whose schema hasn't loaded falls back to
    the wire type (where the date family still resolves and the rest stay text). `fits` is the
    other half and the safety one: a control may only be offered for a value it could itself have
    produced, so a `tinyint(1)` holding `7` and a `DATE` holding `0000-00-00` keep the plain text
    field — a toggle rendered over `7` writes `0` the moment it is touched. An **empty** value fits
    everything (it means "nothing chosen yet"), which is what hands a NULL field its dropdown — and
    that arm answers `true` *before* it looks at the editor, so it is **not** the `ENUM` protection
    this entry used to claim. A MySQL `ENUM` holding the empty string a rejected insert wrote in
    non-strict mode gets the dropdown, and the dropdown has no row that writes `''` back; the cell
    is also indistinguishable from a NULL one, which `start_edit` seeds from the empty string too.
    Telling the two apart means telling `fits` *which cell* it is looking at, which no caller can
    say, so the exception stands and the cost is written down instead: a NULL cell and a fresh
    pending row are the common cases, a literal `''` in an `ENUM` is what a misconfigured server
    produced once. `BoolWire` is which
    two literals a boolean writes, and it is **the engine's own spelling, not the readable one**
    (`1`/`0` on MySQL and SQLite, `t`/`f` on PostgreSQL): every engine accepts several on the way
    in, so the round trip decides — a toggle back to what the column already reads back is
    recognised as a revert and un-stages, where `true` written over a `t` leaves a green cell whose
    `UPDATE` writes a value already there. **The row already *held* is the one exception, and writes
    the text the cell already has** rather than the wire spelling (`pick_options`), because the
    revert property only holds where the engine's read spelling *is* its write spelling. SQLite has
    neither a boolean type nor an opinion about one, so such a column may legally hold `'true'`: the
    picker showed that row as held, and clicking it to confirm staged `1` — a different stored value
    out of an action whose only visible meaning was "yes, that one". Writing back is
    `set_date`/`set_now` (which keep a datetime's other parts — see `date.rs`) and
    `toggle_set_member`, which re-emits a `SET` in **declaration order** because that is the order
    MySQL stores and returns one in.
    **An offset is a property of the destination, not text carried from the old value.**
    `CellEditor::DateTime` carries `Zoned::{Offset, Naive}`, decided by the **type name alone**
    (`zoned_for_type` takes no dialect): only PostgreSQL's `timestamptz` — and its verbose
    spelling — resolves an offset, while a MySQL `DATETIME`, PostgreSQL's bare `timestamp` (which
    parses one and *discards* it) and SQLite's text stamps have
    nowhere to put one. `set_now` asks the destination, where the gate used to be "did the old text
    carry a tail" — which reads `false` for exactly the column that most needs an offset, since a
    MySQL `TIMESTAMP` is rendered in the session zone with no tail at all. So **Now** sent a client
    wall clock the server read as its own: server at `+00:00`, client at `+02`, an instant stored two
    hours in the future and rendered back in the session zone so the cell re-read as correct. It
    drops the old value's fraction for the same class of reason — those were its microseconds, not
    this instant's. `set_date` drops the offset on **both** flavours of column, because an offset
    qualifies a particular instant: `+01` on a Berlin `timestamptz` is true in January and false in
    July, so carrying it onto a picked July day restates the time of day an hour out and changes the
    one thing the user did not touch. **MySQL's `TIMESTAMP` is the one entry this deliberately gets
    wrong**, and it is `Zoned::Naive` on purpose: the type *does* resolve an offset — stored in UTC,
    rendered in the session zone — but there is no offset it can be sent that every server on this
    dialect reads. `[+-]hh:mm` inside a datetime literal is MySQL 8.0.19+, and **MariaDB accepts no
    spelling of it at all**: `+02:00`, `+02`, an ISO `T` separator and `Z` each answer
    `ERROR 1292 (22007) Incorrect datetime value` on MariaDB 10.11.14, and `SqlDialect` is one
    variant for both servers with no version in it. The two failures are not symmetric — withholding
    the offset stores a wrong instant only where the server's session zone differs from the
    client's, while sending one *fails the edit outright* on every MariaDB — which is the same call
    `alter_clauses` makes in emitting `DROP CONSTRAINT` over MySQL 8's `DROP CHECK`. The residual
    divergence is the acknowledged cost, and closing it is not this layer's to do: it needs the
    server's own `@@session.time_zone`, read on connect and carried to the write path, so the wall
    clock can be *converted* rather than annotated
    (`a_mysql_timestamp_takes_no_offset_because_the_dialect_cannot_promise_one` pins the choice).
    The `ENUM`/`SET` member list is parsed off the type text
    through `sql::skip_noncode` — the one boundary lexer — and unescaped as the exact inverse of
    `export::sql_literal`, since splitting on commas loses a member containing one.
  - `date.rs` — civil dates, clock times, and the month grid a picker draws. `Date`/`Time` are
    checked constructions (`0000-00-00` and a `TIME` column's `838:59:59` duration are both
    rejected, which is how `celledit::fits` knows a calendar can't represent them); `Stamp` splits a
    timestamp's text into the parts a picker edits and the parts it must **preserve byte for byte** —
    the fractional seconds, the timezone offset, and whether the source separated date from time
    with a space or a `T`. Picking a day out of `2024-01-15 10:30:00.123456+02` must not quietly
    drop the rest, which is the same silent-rewrite class `jsontree` keeps a number's source text
    for. **The tail is kept without being understood, but not without being checked**: `Stamp::parse`
    validates it as an offset — `Z`, or a sign and one to three digit pairs — and answers `None`
    otherwise, because everything downstream treats *any* tail as one (`has_offset` says so,
    `with_offset` replaces it). A PostgreSQL BC timestamp (`0044-03-15 00:00:00 BC`) came through
    with `tail = " BC"`, so `celledit::fits` said a calendar could show it, the picker opened on
    March 44 **AD**, and *Now* wrote the era away by replacing ` BC` with `+02:00`. The date-only
    spelling was already refused, since `take_time("BC")` fails; the asymmetry was that adding a time
    let the same suffix through. The arithmetic is Howard Hinnant's `days_from_civil`/`civil_from_days` pair — the
    workspace's only date maths, `format.rs`'s epoch formatter included — and `month_cells` is
    **always 42 dates** (six weeks, Monday-first) so the panel doesn't change height as you page
    through the year. `Date::today`, `Time::now` and `local_now` are the module's only impure
    functions and the only use of `chrono` in the workspace: they read the **local** clock, because
    `SystemTime` is UTC and a picker that highlights yesterday between local midnight and the offset
    is wrong at exactly the hours somebody is most likely to be looking at it. **A timestamp field
    goes through `local_now`**, which returns the day, the time of day and the zone's offset from
    **one** reading of the clock: two reads are two instants and local midnight can fall between
    them, which is how the calendar's *Now* — `Date::today` plus `Time::now` — wrote yesterday's
    date beside today's time, a stamp a day in the past from the one control whose whole job is the
    current instant. A caller that wants only the day still wants `Date::today`.
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
      `ai_fix_prompt` is the **one wording of "fix this"**, for all three ways to ask — the editor's
      error bar, the modal behind its *View*, and the right-click *AI fix* over a squiggle. It
      returns the pair the caller needs (the Ctrl+K input line the user sees and can edit, and the
      intent sent with the SQL), so the three can't drift apart in what they ask the model for; the
      `FixOrigin` is what keeps the editor's *warnings* from being announced to the user as "this
      error". The messages ride in a `fenced` block under `UNTRUSTED_NOTE` like everything else here,
      which the error bar's own `format!` did not: a DB error quotes identifiers and cell values from
      a server that isn't necessarily the user's.
      `explain_error_prompt` is its pair, and the two ask for **opposite things on purpose**: a fix
      lands as a diff in the editor with an Approve behind it, an explanation lands as prose in the
      chat panel, where there is no diff and no gate — so this one says outright not to answer with a
      rewrite, or a reply ending in corrected SQL would invite a copy-paste past every check the fix
      goes through. Its statement is **optional**, which is what lets *Explain* be offered for every
      error the modal can show while *AI fix* is offered for one kind: an explanation needs only the
      words, and a commit error, a failed export or a server that never answered are exactly the ones
      whose modal is otherwise a dead end. **Neither prompt consults `AiData`**, unlike
      `result_shape` two paragraphs up, and the difference is consent rather than sensitivity: that
      gate is for text the model is handed *automatically each turn*, while these two carry an error
      the user pressed a button to send — the same election an attached row is.
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
  - `params.rs` — `:name` query parameters: `scan`/`names` (every placeholder and its byte range,
    built on `skip_noncode` so a `:id` inside a string, comment, dollar-quoted body or quoted
    identifier is not one), `bindings_for` (the parameters bar's rows, re-derived from the SQL on
    every edit and carrying across the values already typed), `substitute` (the statement that
    actually runs) and `neutralize` (the same text with each `:` rewritten to `_`, byte offsets
    preserved, so `intel` can parse a statement mid-parameterisation and only the placeholder ranges
    lose their diagnostics). **Named placeholders only** — `?` is a live PostgreSQL JSON operator and
    `$1` collides with dollar-quoting. **Substitution is textual, not driver binding**, because
    `ORDER BY :col`/`LIMIT :n` are what the feature is for and no driver binds those; values are
    quoted through `export::sql_literal`, and `ParamValue::Raw` is emitted verbatim by design.
    That last point is why **the run guard must be given the *substituted* statement** — a `Raw`
    value otherwise carries a write past a `run_verdict` that read a statement the engine never
    receives. `ParamValue` is deliberately not `Serialize`: a value is session-only and never
    reaches `tabs.json`. Three lookalikes have named regression tests, since each is one missing
    line from a false positive: PostgreSQL's `::` cast (consumed whole — skipping one byte leaves
    the shape of a placeholder), MySQL's `:=`, and a PostgreSQL array slice `arr[lo:hi]`.
    `prepare_run` is the pair the run action calls — substitute, then `sql::run_verdict` on the
    result — so the ordering is structural rather than a rule a caller has to remember, and
    `strip_param_diagnostics` drops the reports `neutralize`'s own rewrite caused. Pure +
    unit-tested.
    The UI half: `Tab::params` is the bar's store (session-only — no `SavedTab` field, and
    `ParamValue` has no `Serialize` derive), `editor_pane::params_bar` renders one row per distinct
    name under the editor, and `guarded_run`/`guarded_run_all` in `schemaic-app` substitute with it.
    Rows are keyed by **name**, not value, so typing into a field never rebuilds the field being
    typed into. **Tier-2 live validation is skipped for a statement holding a placeholder**, and
    `intel::parses` does not cover that: `sqlparser` accepts `:id` as a placeholder in all three
    dialects, so the statement parses, the round-trip happens, and the squiggle is the *server*
    complaining about text the user is still filling in. A placeholder in an identifier position
    (`FROM :tbl`) is the case that does not parse — which is what `neutralize` exists to rescue.
  - `script.rs` — reading a `.sql` script **back**, as a stream of statements. The counterpart to
    `dump.rs`: that module decides what a replayable file holds and hands the app a plan to write
    it, this one takes such a file apart a block at a time, so a dump far larger than memory can be
    replayed. Until it existed the round trip did not close — Schemaic wrote `.sql` files only
    another tool could read, because the import path takes CSV, JSON and Excel and
    `sqlfile::open_verdict`
    refuses a `.sql` past 64 MB.
    **What a statement is and what a server may be *sent* are two different strings**, and
    conflating them shipped a bug: a range carries its terminator — right for the editor, which
    selects and highlights with it — but when that terminator is the client's `DELIMITER` token it
    must come off. `END$$` lexes as a single identifier in MySQL, so the compound body never closes;
    every dump carrying a trigger or routine failed at its first one, half-loaded, on *both* this
    runner and Run Everything (which had it all along). `sql::Bound` now records how many of a
    boundary's bytes are the client's, `;` keeping `strip: 0` because every engine accepts a trailing
    one, and `sql::executable_statements` / `executable_range` are the single answer to "what is
    sent" that both paths ask. Confirmed against MariaDB: `CREATE TRIGGER … END$$` is a 1064,
    `… END` creates the trigger.
    `Splitter` is the whole of it: `push` takes the next block's **bytes** and returns the
    `Statement`s it completed, `finish` yields the last one (a script's final statement need not
    carry a terminator, and a runner that dropped it would replay a dump one statement short,
    silently). The splitting itself is `sql::statement_bounds_open` — the one boundary lexer, made
    resumable — so what this module owns is the *discipline* around it: **it only ever drains up to
    a boundary the scan actually found.** That single rule is what makes a block boundary landing
    inside a string, a comment, a dollar-quoted body or a `DELIMITER` directive a non-event: an
    unterminated construct yields no boundary, so those bytes stay in the buffer and are re-scanned
    from a real statement start when the rest of the file arrives.
    **`push` takes bytes rather than `&str` deliberately.** A block boundary lands on a byte offset,
    which on a UTF-8 file is often mid-character, and a caller left to solve that itself is one that
    will eventually call `from_utf8_lossy` per block — turning a character straddling the boundary
    into two replacement characters *inside a string literal*, silently, on its way to the server.
    The split sequence is held here and completed by the next block; a genuinely invalid byte costs
    one `U+FFFD`, which is `sqlfile::decode`'s answer to the same question.
    Each `Statement` carries the `line` and `offset` it started at, because the failure this has to
    report is "statement 30,000 of a file you cannot open in the editor". Newlines are counted once
    walking forward with the bounds rather than re-counted per statement, which would be quadratic
    in a block holding thousands of small `INSERT`s. `pending` is the driver's backpressure signal
    and `MAX_PENDING_BYTES` its ceiling: a file that reaches it is not a script with a long
    statement, it is a file with no terminator in it at all.
    `probe` is the other half, and is `import::read_sample`'s counterpart: what the Import modal's
    second step shows once a file is picked. A CSV's sample can show the opening *rows*; a script's
    can only show what its opening statements **do**, which is the thing worth knowing before
    running one — a kind histogram (`INSERT ×400`, `DROP TABLE ×12`), how many statements destroy or
    delete data (`is_destructive`, and see the write-guard invariant for why that count *is* the
    confirmation and why its net therefore includes `DELETE`), and whether the file opens its own
    transaction (`dump.rs`'s *Replaying → One
    transaction* put one there, and the runner must not wrap an already-wrapped file). It is bounded
    by `PROBE_MAX_BYTES` — the same 8 MB as `SAMPLE_MAX_BYTES`, for the same reason: the user asked
    to *look* at a file — and by `PROBE_MAX_STATEMENTS`. Either bound sets `Probe::more`, and every
    count is then reported through `count_label` as a floor (`400+`), never rounded up to a total
    that would have cost a full read to know. **A truncated read drops the statement it was cut in
    half**: half an `INSERT` classifies as an `INSERT`, and `Splitter::finish` means "the file ended
    here", which is not true of a byte the probe merely stopped at.
    `statement_kind` is the vocabulary — the verb, plus the object for `CREATE`/`DROP`/`ALTER`,
    where `CREATE TABLE` and `DROP TABLE` are not the same news. The object list is a **whitelist**,
    so an unfamiliar shape degrades to the bare verb rather than to a guess: the alternative rule,
    "skip the modifier words", is a list of every modifier three engines have, wrong the first time
    a version adds one and wrong silently. The case that settles it is MySQL's view preamble
    (`CREATE ALGORITHM=… DEFINER=…@… SQL SECURITY DEFINER VIEW`), which puts `VIEW` eighth once the
    back-quoted account is skipped as an identifier — and which is what sets the floor on
    `sql::leading_words`' bound. That function is `leading_keyword`'s plural and is bounded on
    purpose: the private `word_tokens` tokenises to the end, which on a dump's sixteen-megabyte
    extended `INSERT` allocates a `String` per word to learn one.
    `run_outcome(ReadEnd, ExecEnd, ran)` is the two halves' ending split, and it is `dump.rs`'s
    `dump_verdict` **inverted rather than copied** — the one thing to know before touching it. In a
    dump the *database* reads and the *file* is written, so a full disk fails the writer and the
    reader only ever learns "nobody is reading any more"; the writer therefore describes it. In a
    load the halves swap: a file that stops being readable is invisible to the executor, which sees
    a channel close it cannot tell from a script that simply ended. So here a **reader** failure
    outranks even the server error it caused — a truncated read hands over half a statement, the
    server answers "syntax error near…", and reporting that would name the file's last line as the
    problem when the disk is. Cancel is either half's to witness and neither's to call a finish.
    **Every arm carries `ran`**: a script is not transactional unless the file said so, so "it
    stopped" always leaves "how much of it happened?", and that count is the only answer — which is
    why an exhaustive test asserts no arm drops it.
    **The write guard for this path is `sql::script_verdict`**, not `run_verdict` — see the
    invariant.
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
    - `resultsel.rs` — the same rules one strip lower: which **result** panel is shown, and which
      ones a run is allowed to replace. `after_run` is the feature in one function — the pinned
      panels, in their order, then the run's fresh ones — and `active_after_run` answers the other
      half: a run shows *its own* first result, never the pin that survived it, because a query that
      appears to do nothing is worse than one that scrolls the strip. `pin_order` keeps the pinned
      block contiguous at the front (the invariant every other rule here is a filter rather than a
      sort because of), and `can_close`/`all_to_close`/`others_to_close`/`active_after_removal` are
      `tabsel`'s closing rules restated for panels — deliberately the same answers, since the two
      strips sit one above the other and a user who has learned one has learned the other.
      `active_after_removal` serves all three closes at once (one, others, all): the active panel
      when it survives, else the nearest survivor to its right, else to its left. Written as one
      function because three spellings of "where does the strip land" is three chances to disagree.
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
      (`"1.2k"`) can be displayed while the singular/plural decision still follows the true `n` —
      a contract that reads as "returns the phrase" if you skim it, and the export note shipped as
      `Exported rows to employees.csv` on exactly that misreading, so a call site here owes the
      count as well as the noun. `human_count` (`1250` → `1.25k`), the **row-count** printer, shared
      by the grid's stats line and the properties surface so `200k` means one thing — and bound by a
      round-trip property: every string it emits must parse back through `model::goto_row_index`.
      Not the namesake in `transcript.rs`, which buckets token counts differently on purpose.
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
      - `rows_read_clause` is the toolbar's whole row segment — figure, noun, and the `(capped)`
        notice — and the reason it is one function rather than three fragments the view assembles
        is that only the composition can decide whether the notice is still needed. Its private
        half `rows_read_of` prints `1k` alone or `1k of ~4.2m`, dropping a total at or below what
        was already read (`1k of ~400` reads as a bug rather than as the stale estimate it is);
        **whether that comparison got printed is then what silences the word.** A total is in hand
        only for a capped read — `grid_view`'s `scanned` is gated on `truncated` before the
        catalogue is asked — so `200k of ~292.02k rows` cannot mean anything but a read that
        stopped short, and `200k of ~292.02k rows (capped)` spent nine characters restating it on
        a strip already wide enough to push its own buttons off a narrow panel. The word stays
        wherever there is no comparison to make. **That premise is now enforced here rather than
        assumed**: the function drops the total outright when `truncated` is false. The gate that
        guaranteed it lives in a view closure no test can reach, and fetching the total
        unconditionally is the obvious optimisation — the tree already holds it — so the moment
        anyone takes it, `42 of 1,000 rows` would appear over a `SELECT … WHERE` that legitimately
        matched 42 of 1,000: a claim that 958 rows were withheld, with no `(capped)` to hint a cap
        was ever involved. The noun follows the last figure named, which is
        the total when there is one: `1 of ~4.2m row` and `0 of 1 rows` are both wrong.
        `truncate_prompt`/`drop_prompt` are the destructive confirmations'
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
  **`Db::fetch_principals`/`Db::fetch_grants`** are the Users and privileges browser's backend, and
  they gate on `users::supports_users` before the engine `match` for the same reason those two do,
  with `NO_USERS_MSG` as the one sentence both raise. The MySQL half is `collect_my_users`: **four
  queries, of which exactly one runs to completion** — MariaDB's `mysql.user` column set, then MySQL
  8's, then the bare `(User, Host)` pair, then `information_schema.USER_PRIVILEGES`. **The fallbacks
  fire on an *error*, not on an empty result**, the rule the lock-wait pair above had to learn:
  `mysql.user` is never legitimately empty, so an empty answer would mean the read was denied and
  only the error says so. The last query is the one whose failure the caller sees, and it is the one
  an *application* account can actually read — `mysql.user` needs `SELECT` on the `mysql` database,
  which a properly-provisioned account has not got, while `USER_PRIVILEGES` shows it its own row: one
  account is a poor list, but it is a true one and it is the account the person opening this is most
  likely asking about. `my_user_rows` only slots the fifth column into `is_role` or `account_locked`
  by which query answered; everything else is `users::from_mysql_rows`. `fetch_grants` runs
  `SHOW GRANTS FOR <account_sql>` and pipes **every row through `users::redact_secrets` at the
  boundary**, not in the view, so a second caller — the grant/revoke step beside it, a copy
  button — cannot forget to. `pg::fetch_principals` reads `pg_roles` on the **maintenance**
  connection (the catalogue is cluster-wide and the browser may be open with no database selected)
  and, unlike `pg::roles` behind the Owner dropdown, **keeps** the `pg_` predefined roles, since they
  hold real privileges and `users::from_pg_rows` sorts them last. `pg::fetch_grants` is **four
  queries, because PostgreSQL has no `SHOW GRANTS`**: `pg_auth_members`, then `aclexplode` over
  `pg_database.datacl`, `pg_namespace.nspacl` and `pg_class.relacl` (relkinds `r p v m f S`, filtered
  rather than left open so an index or a composite type can't reach the statement builder, and `S`
  the only one taking the `SEQUENCE` keyword). Three of the four see **one database only**, which is
  what `users::pg_scope_note` says on screen. Every one of them compares against the role's own oid,
  so a role dropped since the list was fetched yields an empty answer rather than an error. `pg_bool`
  is the small reader `simple_query`'s text protocol needs, which spells a boolean `t`/`f`.
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
  **One read reports one result set, and both engines now agree on that.** A PostgreSQL simple query
  is one string and the server will happily run every statement in it — the MCP server hands one
  straight through and nothing upstream splits it — so `pg::run_statement` counts the
  `RowDescription` messages, the only boundary marker on that stream, and stops at the second. It
  used to fall through the `_ => {}` arm that `SimpleQueryMessage`'s `#[non_exhaustive]` requires, so
  the second set's rows were pushed through the *first* set's columns and cell kinds: reproduced live
  through `Db::fetch_query`, a two-statement string came back as 3 rows under one column header, the
  third being 1,282 characters of hex from a `bytea` read under set one's `INT4` — the case `pg_cell`
  exists to prevent. That is the answer MySQL's `collect_rows` has always given by construction, and
  breaking out is safe for the same reason the row cap's `break` is: the driver's connection task
  keeps paging, so the connection stays reusable, which a Manual-mode tab depends on. The
  non-exhaustive arm stays, and the question to ask of anything new arriving there is whether it
  carries a boundary or a column list, because those are what the loop's state rests on. The
  column-metadata `PREPARE` describes `sql::first_statement(sql, PG)` rather than the whole string
  for the same reason: `Parse` takes a single command, so a multi-statement string cannot be prepared
  at all, and a failed prepare leaves every `type_name` empty — an empty type name is an unknown
  `CellKind`, which is how the `bytea` came to be read as text in the first place. For a single
  statement it is the same string and the same one round trip, terminator included.
  `fetch_query`/`stream_query`/`run_batch`/`fetch_schema`/`ping`/`commit_writes`/`refetch_rows`/
  `prepare_check`
  (non-executing `PREPARE` for the editor's live validation)/`run_ddl`/`run_script`/`fetch_table_stats`/
  `count_rows` are `Db` methods taking the target DB per call.
  **`Db::run_script` is `stream_query` inverted** — it takes a bounded
  `Receiver<script::Statement>` where that one takes a Sender — and it is the load half of the
  round trip `dump.rs` opens. It executes each statement in order on one pinned connection, stops
  at the first the server refuses, and returns `(script::ExecEnd, ran)` for the driver to fold
  against how the *reading* half ended. Three arms, all of them minimal by design: no transaction
  of ours on any engine, and no pragma on SQLite. See the connection invariant for why it can be
  neither `Session` nor `run_ddl`, and `core::script` for the module it serves. Cancellation is
  selected on **while waiting for the next statement** as well as while one runs — a load stalled
  on a slow disk spends most of its life waiting, and a Stop that only landed between statements
  would look ignored.
  **`Db::stream_query` is the whole-table export, and the row cap is the thing it exists to
  escape**: it runs `sql` uncapped and hands the rows to a *bounded*
  `tokio::sync::mpsc::Sender<ExportChunk>` in blocks of `chunk_rows` as they arrive, returning how
  many went out. `ExportChunk` is `Result<ResultSet, String>`, and **the error rides the channel as
  well as being returned**, because the writer is on the other end and would otherwise read a
  closed channel as "the table ended" and call a half-written file finished. It shares the engine
  dispatch with `fetch_query` through the private `Db::run_to` — one connection, one statement, one
  destination for its rows — and that destination is `RowDest`: `Capped(n)` for every ordinary
  query, `Chunked { chunk, tx, sent }` for a stream, whose `cap()` is `usize::MAX` so no row loop
  grows a second branch around the comparison it already makes. **Streaming is a second destination
  for the rows, not a second way to read them.** Each engine has exactly one row loop, each the
  product of a long argument with its driver, so `RowDest` is threaded through them as a parameter
  rather than three more loops to keep in step; every previously-capped call site — `Db::fetch_query`,
  every arm of `run_batch`, `explain`, `pg::fetch_table`, `Session::fetch_query` on both backends,
  and SQLite's `run_query` — simply passes `&mut RowDest::Capped(n)` and behaves exactly as it did. It *owns* its `Sender`
  rather than borrowing one, because SQLite's loop is moved into a `spawn_blocking` that needs
  `'static`, and that same split is why there are two flush methods and not one: `flush` awaits
  (MySQL, PostgreSQL) while `flush_blocking` calls `blocking_send`, which is right inside
  `spawn_blocking` and panics on a runtime thread. All three loops flush the tail
  **unconditionally for a row-returning statement, including an empty tail**, since the export's
  header comes from the first chunk and a table with no rows has only that block.
  **A statement with no result set is a refusal, not an empty export.** All three return *before*
  that flush when the statement yields no columns at all (a DML/DDL/utility outcome reports
  `affected` instead), so nothing whatever reached the channel — and the writer, seeing no chunk,
  produced an empty file and reported it as `Done(0)`. `stream_query` answers
  `DbError::Query("that statement returns no rows to export")` when the run comes back with no
  columns and nothing sent, and puts it on the channel as well, like every other error here. The
  refusal lives in `stream_query` rather than in the caller that happens to be careful — the export
  menu never offers such a statement — because this is public API and the next caller may not be
  gated the same way. The four tests are in `sqlite.rs`, over in-memory
  SQLite (`a_streamed_query_ignores_the_cap_and_arrives_in_bounded_blocks`,
  `an_empty_table_still_streams_the_block_that_carries_its_columns`,
  `a_failed_stream_sends_its_reason_down_the_channel`,
  `a_statement_with_no_result_set_is_refused_rather_than_exported_empty`).
  **Uncapped changes what a per-cell cost means, and three things came out of that.** A chunk ends
  by rows **or by bytes**: `RowDest::chunk_full(rows, bytes)` also cuts at `CHUNK_BYTE_BUDGET`
  (32 MiB, read off `ResultBuilder::text_bytes`), because a block is `chunk × the row width` and
  nothing bounded the width — the channel holds two, the loop is filling a third and the writer is
  rendering a fourth, so a table of 1 MB documents put ~40 GB in flight against a constant whose own
  doc promised "megabytes rather than gigabytes". That was a claim about bytes made by a figure
  counted in rows; the only thing that had ever stopped it was the per-column 512 MiB arena ceiling,
  and hitting *that* is the loss `export::ExportTally::blanked` now reports. The budget bounds the
  arena and not the whole `ResultSet`, whose one offset word per cell is proportional to
  `rows × columns` and already bounded by the row count. Then the two type hoists: `db::NumKind` /
  `num_kind` / `parse_as` split `parse_typed`'s *decision* from its *parse*, since the decision is a
  `to_ascii_uppercase()` (a heap allocation), six `starts_with` probes and, for integers, a
  `contains("UNSIGNED")` scan — nothing at all per column and 100M allocations on a 5M × 20 export.
  A row loop calls `parse_as` with a kind computed once per column, `parse_typed` stays for the
  callers with a single cell to convert, and it **is** the composition of the other two, so the two
  spellings cannot drift. `pg::cell_kinds` is the same hoist on PostgreSQL and answers the numeric
  kind and the binary flag **together**, once per column, replacing a per-cell
  `to_ascii_uppercase()`.
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
  panel goes back to.
  **`fetch_schema` takes one too**, and it was the last counter-example to that rule. It reads every
  column, index, key, view, check and trigger of a whole database, so on a few hundred tables it runs
  for a long time — and the Export modal mounts its `Reading the schema` phase over it behind a full
  backdrop whose only exit is a cancel, so Stop did nothing at all and `app/dump.rs`'s
  `Err(DbError::Cancelled)` arm was unreachable code. Each engine cancels its own way: MySQL races
  `collect_schema` against the token and `KILL`s through a second connection, PostgreSQL races the
  catalogue reads — split out of the connect into a private `pg::collect_schema` for exactly that, so
  the whole sequence is raced rather than a token checked between each of a dozen queries — and calls
  `cancel_query` on the server, and SQLite checks **at the door and no further**, the read being
  local `sqlite_master` and `PRAGMA` traffic against a file with no server to leave a query running
  on. `Db::fetch_schema` refuses at the door as well, before any engine opens anything: a token
  cancelled before the call would otherwise pay for a connection handshake and then a *second*
  connection to `KILL` a query that was never issued. A caller with no Stop of its own —
  `app/main.rs`'s tree refresh, the three `mcp.rs` sites — passes `CancellationToken::new()`, which
  is never cancelled. `run_ddl` is the schema-editing apply path and is **honest about
  atomicity**: PostgreSQL runs the whole plan in one transaction (transactional DDL), MySQL runs
  it sequentially and reports which statement failed *and how many already stuck*
  (`DdlError::applied`) — every MySQL DDL statement commits implicitly, so a transaction there
  would be theatre. **`applied` counts what *applied*, which is not what succeeded**
  (`ddl::applied_count` over `ddl::alters_the_database`, whose whole test is that the leading
  keyword is not `SET`): a MySQL routine, trigger or event edit is emitted inside a session guard,
  and every one of those wrapper statements sets a session variable on a connection the runner
  disconnects a line later. Counting them had a rejected `ALTER EVENT` report *2 earlier statements
  already applied and cannot be rolled back* when nothing had been — over the app's only disclosure
  of a genuinely half-applied migration, which is the one number here that must never be inflated.
  The decision is in `core::ddl` rather than a counter in the runner because it is a decision about
  emitted SQL and the runner has only strings, which is exactly how the scaffolding came to be
  counted; `a_session_guard_is_not_a_statement_that_applied_anything` composes it with the emitters,
  so an emitter that starts producing a real `SET` fails there instead of quietly under-counting.
  **`DdlError::at` deliberately did not change**: it stays an ordinal over the whole emitted plan,
  scaffolding included, because that is the script the preview panel shows and a "statement 3" that
  disagreed with what is on screen would be a second wrong number rather than a fix for the first.
  **SQLite's DDL is transactional too**, so `sqlite::run_ddl` wraps the whole
  plan in one and rolls it back whole, which is why every `DdlError` from that backend carries
  `applied: 0` — a half-applied plan is a state this engine never leaves behind, so there is no
  partial progress for the report to admit to (`sqlite::ddl_tests`, over in-memory SQLite). That
  arm no longer refuses every plan: the gate moved upstream to `ddl::supports_change`, which can
  see the `Change` where `run_ddl` has only strings, and refusing wholesale here would have taken
  away the drops SQLite genuinely has.
  **`run_server_ddl` is the second apply path, and it exists because two statements can take
  neither of `run_ddl`'s commitments.** `CREATE DATABASE` and `DROP DATABASE` — the pair
  `ddl::is_server_level` marks — cannot run on a connection to the database they are about (one
  does not exist yet, the other is what PostgreSQL means by *"cannot drop the currently open
  database"*), and PostgreSQL refuses both **inside a transaction block**, which is precisely
  what `pg::run_ddl` wraps every plan in. So this path connects without naming the target and
  runs untransacted, and nothing is given up by either: a server-level plan is one statement, so
  there is no second one for a rollback to protect and no scaffolding for `applied` to discount.
  **On MySQL "without naming the target" is `open_serverless`, not `open(None)`**, and the two were
  the same spelling for a release. `Db::open(None)` used to mean *"this operation needs no database
  scope"*; since a connection gained a configured **Database** it means *"the caller named none, so
  use the connection's"* — so `DROP DATABASE shop` on a connection configured for `shop` went out
  on a session pointed at its own target, and every later operation answered `ERROR 1049`. The two
  readings now have two names (`Scope::Database` / `Scope::Server`), which is the only thing that
  keeps them apart: eleven call sites were written under the first reading and most are harmless
  only because their SQL happens to be qualified.
  The PostgreSQL arm takes the target through `connect_maintenance_avoiding`, since the
  *configured* database is the first maintenance candidate and dropping it would otherwise pick,
  before anything else, the one connection that cannot perform the statement
  (`the_database_being_dropped_is_never_the_one_it_runs_on`, which covers the username guess too
  — role and database sharing a name is the default PostgreSQL setup). SQLite's arm is an error
  with a sentence in it rather than a statement: a database there is a file, so creating one is
  the connection form's business and dropping one would be deleting the user's file off disk.
  Which path a plan takes is **decided where the `Change` still is a `Change`** — `preview_of`
  stamps `DdlScope` onto the preview from `ddl::is_server_level`, `ddl_preview::apply` passes it
  through on the `DdlRunRequest`, and `app/main.rs` branches on it. Asking "is this a `CREATE
  DATABASE`?" of a `Vec<String>` would be the hand-rolled scanner this codebase keeps out.
  **The refresh branches with it**, and that is not bookkeeping: what a server-level plan changed
  is the connection's *list* of databases, so it calls `refresh_schema` (re-list) where an
  ordinary plan calls `refresh_db(database)`. Left on the latter, a drop re-introspected a name
  that no longer exists and a create re-introspected one the tree had never heard of — both find
  no node and return, leaving the tree showing a database that is gone.
  **An account plan needs a third refresh on top of whichever branch it took.** It runs the
  `Database` route (`ddl::is_account_change`), whose refresh re-introspects a *schema* and knows
  nothing about `mysql.user` — so a created account never appeared and a dropped one stayed on the
  list until the browser was closed and reopened. `app/main.rs` therefore re-fetches the account
  list after a successful apply when the browser is open, and puts the selection back to **nothing**
  rather than keeping it, since the account it named may be the one that was just dropped.
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
  **Scheduled events read the same way and degrade differently.** `MY_EVENTS_SQL` /
  `my_event_row` → `my_event_row_from` / `MY_EVENT_COLUMNS` are the routine trio again, sixteen
  columns instead of fourteen and for the same arity reason; `mysql_events` folds the rows,
  deciding the schedule's shape from `EVENT_TYPE` rather than from "is `EXECUTE_AT` NULL" (which
  is also NULL for a recurring row whose interval failed to convert), and quoting the timestamps
  and the quantity into the SQL expressions the model holds. A `RECURRING` row with no readable
  interval becomes `EVERY 1 DAY` rather than being dropped: an event Schemaic can't fully describe
  is still one the user must be able to see, rename, disable and drop — and the `ONE TIME` arm
  falls back the same way, to `CURRENT_TIMESTAMP`, because an empty `AT` is exactly what
  `EventDraft::validate` refuses and would have left Preview disabled for the life of the modal.
  Neither fabrication can reach the server on its own: `event_alter_clauses` restates `ON SCHEDULE`
  only when it changed, and it hasn't until the user edits the field they are looking at.
  Unlike the `TRIGGERS` read
  beside it, a **missing `information_schema.EVENTS` degrades to an empty list** on 1109/1146 — the
  same two codes `CHECK_CONSTRAINTS` retries on, and for a sharper reason: the MySQL-protocol
  servers that aren't MySQL are exactly the ones that may not implement the scheduler, and a
  database whose tables can't be browsed because it has no events catalogue is a far worse outcome
  than one whose Events folder is empty. `Db::event_source` is the sixth text divergence;
  `event_body_of` is its pure reader and is the simplest of the three, since `DO` is a keyword it
  can anchor on — what it still needs is `sql::skip_noncode`, for the event named `` `do` `` and
  the `COMMENT 'do not touch'` that both sit before the real one.
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
  connection. **A long read blocks a write here, and on neither other engine** — SQLite takes one
  write lock over the whole file, so a whole-table export or import holds a write off until it
  finishes. Measured against a real 53 MB file in rollback-journal mode (`journal_mode = delete`,
  the default for a file not already in WAL): a `stream_query` over 400,000 rows drained slowly,
  with an `UPDATE` on a second connection issued a quarter-second in, **waited 3,495 ms and then
  succeeded** when the export finished inside the busy timeout and **failed after 5,536 ms** when it
  did not. **How long a write waits is this app's number, not the driver's**: `open` sets
  `busy_timeout` to 15 s explicitly, where rusqlite's own 5 s was an implementation detail the app's
  behaviour rested on — both figures above are that default, and both move if it changes. Fifteen,
  because the band between 5 and 15 is where a wait actually *resolves* (a chunk flush, a commit, an
  import's transaction), while past it the blocker is a read as long as the table is big and waiting
  minutes with no way to cancel is worse than a refusal that says why. That refusal is
  `query_err` appending `LOCK_ADVICE` on `SQLITE_BUSY`/`SQLITE_LOCKED`, read off `ErrorCode` and
  never matched in the message text (`is_lock_failure`) so a reworded SQLite build cannot quietly
  drop it: the engine's own sentence is *database is locked*, which names neither the user's other
  operation nor the way out, and someone who has only met two MVCC engines has no reason to connect a
  failed cell edit to the export running in another tab. **WAL is named rather than applied** — the
  advice spells out `PRAGMA journal_mode = WAL` and says Schemaic will not run it, because
  `journal_mode` is a persistent property of the user's file, it adds `-wal`/`-shm` siblings, and it
  is unavailable on some filesystems; changing someone's database as a side effect of a failed cell
  edit is not this layer's call. `db::lock_wait_sql` answers the empty string here for the matching
  reason: SQLite has no lock-timeout *setting*, the wait being this per-connection busy timeout, so
  the failure that statement bounds elsewhere has no analogue. **Values are dynamically typed**: a declared type is an *affinity*, so `value_of`
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
  **PostgreSQL cannot connect without naming a database**, which is protocol rather than
  preference and stayed invisible while almost every server had a `postgres` one anybody could
  reach — so `connect_maintenance` guessed at that, the username and `template1` for server-level
  work. A hosted provider need not publish any of them: on Aiven only `defaultdb` is permitted,
  every guess is refused by `pg_hba`, and the server was unreachable with nothing in the form to
  correct it (Test connection, the database list and the schema tree all route through that probe).
  `Connection::database` is the fix, read through `default_database` so SQLite — whose file *is*
  its database — answers `None` however the engine picker left the field. It is the probe's
  **first** candidate rather than its only one, since a database since dropped should degrade to
  the old guesses instead of locking the user out of the server; `maintenance_candidates` holds
  that order, deduplicated and without an empty username, as a tested decision rather than a
  literal inside an I/O loop. On MySQL the same field is the database a connection opens in when
  none is selected, and a *fallback* rather than an override — an operation that named one is
  working in it. MySQL stays strict where Postgres cannot be: a database that does not exist fails
  with `Unknown database 'x'`, which beats silently opening nowhere, and Postgres has no such
  option because it must connect somewhere. The probe also **stops early**: only a database-level
  failure (`3D000`, or `28000`, which reads as an auth error but is matched per database) is worth
  another candidate, because repeating a rejected certificate or a wrong password once per
  candidate is how a misconfigured certificate came to report "timed out" rather than naming
  itself.
  **`tls.rs` translates a connection's TLS settings into the two networked drivers**, and only
  translates: the decisions were already made by `core::connection::Tls::plan`, which collapses the
  five libpq `sslmode` levels into the four booleans a handshake is actually made of. The drivers
  spell those very differently — `mysql_async` takes two `danger_*` toggles on an `SslOpts`, while
  `tokio_postgres` takes an `SslMode` for the negotiation and leaves every verification question to
  the rustls `ClientConfig` behind it — and deriving each from the five modes separately is how
  `verify-ca` comes to mean one thing on MySQL and another on PostgreSQL. Three things here are
  load-bearing. **The verifier ladder**: rustls checks the chain and the host name in one call, so
  `verify-ca` cannot be *configured*, only wrapped — `NameAgnosticVerifier` is the real webpki
  verifier forgiving exactly `CertificateError::NotValidForName`, because a wrapper that accepted
  everything on any error would turn `verify-ca` into `require` wearing a stronger label, silently
  admitting expired and unknown-CA certificates. **The tunnel's name**: `Db::connect` rewrites the
  endpoint to `127.0.0.1:<local port>`, which would have `verify-full` compare a perfectly good
  certificate against the loopback address, so the name to check is carried over in the same step
  that moves the address (`TlsPlan::hostname_override` → `FixedNameVerifier`, and MySQL's
  `with_danger_tls_hostname_override`) — substituting the name rather than skipping the check,
  which would make `verify-full` mean `verify-ca` for every tunnelled connection. **The `prefer`
  retry**: a server without TLS fails the handshake rather than declining it, so "encrypt if you
  can" exists only as a second attempt in `Db::open`, and offering that second attempt to anything
  above `Prefer` would let the strongest half of the setting report success in plaintext. **The
  retry inspects the error**, and that is not a detail: its only condition used to be
  `fallback_to_plaintext`, so `prefer` retried after a wrong password (twelve connect attempts for
  ten pings, measured) and after anything an attacker can provoke mid-handshake — one injected RST
  and the whole operation was in cleartext. `should_retry_plaintext` matches the one variant the
  mode exists for, `NoClientSslFlagFromServer`.
  PostgreSQL needs no such retry — its driver negotiates the downgrade itself — which is why the
  mapping is `pg_ssl_mode`, not a shared retry. Cancellation goes through `pg::cancel_query` rather
  than a bare `NoTls`: cancelling opens a *second* connection, and a plaintext one is refused by
  exactly the servers this setting exists for, with the failure discarded. That is a property of a
  **set of call sites**, not of the function — the function was right the whole time two of the ten
  sites bypassed it — so the gate is `session::pg_cancel_gate`, which scans the crate for a
  `cancel_query` outside the module that defines the helper. Both cancels are bounded by
  `CANCEL_TIMEOUT`: best-effort and unbounded are not the same word, and a Stop against a host that
  has gone away hangs inside a modal whose every exit maps to that same Stop.
  **Two things the ladder cannot deliver, said out loud rather than left to a connect.**
  `verify-ca` on MySQL/MariaDB also rejects a host-name mismatch: `mysql_async` 0.37 implements its
  skip-domain-validation toggle by matching `"NotValidForName"` in the verifier's error *text*, and
  rustls 0.23 raises `NotValidForNameContext`, whose `Display` has no such substring — so the two
  verifying rungs are one mode there. `SslMode::caveat` puts that under the picker, and
  `the_driver_still_reads_the_verifier_error_by_its_words` pins what is true **now**, so it turns
  red the day the drivers agree again. And a **passphrase-protected client key** is refused by name
  (`parse_key`), with the `openssl pkcs8` command to fix it: the form used to collect a passphrase
  for one and hand it to nobody, so an encrypted key failed with "is not a PEM private key",
  blaming a file that was perfectly good. `read_certs` refuses a file with **no PEM section** for
  the same reason — a DER `.crt` is what Windows' *Export certificate* writes, it came back as
  `Ok(vec![])`, and `preflight` passed it while PostgreSQL rejected the identity and MySQL
  presented it.
  **How any of this is verified, since none of it can be tested purely:** the unit tests here cover
  the decisions and nothing pure covers a handshake. `db/examples/tls_matrix.rs` walks every mode
  against a live server and prints what each one did, and `scripts/tls-testbed/` is what it is
  pointed at — a local CA and a deliberately-broken certificate set (`wrongname`, `expired`,
  `otherca`), because a real hosted endpoint gives you exactly one case, the happy path, and cannot
  serve an expired certificate because you asked. `swap-server-cert.sh` moves the server from one
  to the next; the README carries the matrix the harness reproduces.
  **The harness reports the transport, not just the connect**, and that is the whole of its
  usefulness: a `ping()` says only "the server let me in", and printing that for a plaintext
  connection and a TLS one alike would have reported `prefer` falling back after a wrong password
  as a pass — so every successful cell asks the server (`Ssl_cipher`, `pg_stat_ssl`). Its negative
  control (`TLS_WRONG_CA`) has no derived default for the same reason: rewriting `ca.crt` in
  `TLS_CA` is a no-op for any other file name, so column B silently became column A while the
  matrix read clean. The test-bed's accounts are `localhost`-only, scoped to one sandbox database,
  and dropped by teardown: the README prints their password, and it is still a credential.
  **Trust anchors are the OS store** (`default_roots`, `rustls-native-certs`), read **once** —
  one-connection-per-operation puts this on the path of every query and health poll, and
  enumerating the Windows store per connection would be a cost paid thousands of times for an
  answer that cannot change mid-session. `webpki-roots` remains only as a fallback for a machine
  with nothing readable, and as a *fallback* rather than a union on purpose: reading the OS store
  is worth doing because an administrator's decisions there bind, so a CA they removed has to stop
  being trusted. **The workspace's `webpki-roots` must stay on `mysql_async`'s major**, and that is
  a real constraint rather than tidiness: the bundled arm is the one case where the driver's own
  built-in roots are deliberately left switched *on*, because they **are** our set — which was
  false while the workspace held 0.26 and the driver 1.0, so the two engines verified against two
  Mozilla snapshots under a promise of one. Nothing in the crate can see that; the guard is the
  dependency. Keeping the two engines on one set is the part that takes work — `mysql_async`
  adds its own compiled-in roots to whatever it is given, so `mysql_ssl_opts` sets
  `with_disable_built_in_roots` and passes the anchors as raw DER buffers, which its loader
  accepts only because DER trips no PEM section (pinned by a test, since it is an assumption about
  someone else's parser). That switch is deliberately **not** made for `prefer`/`require`: the
  driver builds a `WebPkiServerVerifier` even when it will never consult it, and that builder
  fails on an empty root store, so disabling the built-ins for a mode that names no CA would break
  the default mode of every connection. Two things to know before blaming a server. The anchor
  count is legitimately smaller than the certificate manager's — `rustls-native-certs` returns only
  server-auth roots, and a Windows box measured here held 69 and yielded 37, the rest being
  code-signing and timestamping CAs, with `ISRG Root X1` among those kept — so a low number is not
  itself a symptom. Separately, Windows populates its root program lazily, so a certificate every
  browser accepts can still fail `verify-ca` with `UnknownIssuer`, and one installed during a
  session needs a restart to be seen.
  `import_rows` is the bulk-load path (both engines): one transaction of batched multi-row
  `INSERT`s pulled from a `RowSource` iterator, each batch required to affect exactly as many rows
  as it carried — the `commit_writes` 1-row safety net scaled to a file, without its
  statement-per-row round-trips.
  **`tests/live/` is the DB layer against real servers**, and it exists because the pure suite can
  only reach the *decisions*: SQLite is testable directly (in-memory, shared-cache), so it is the
  one backend whose wire layer was covered at all, while MySQL, MariaDB and PostgreSQL — the
  engines that ship most — were covered by hand. `main.rs` holds a macro that expands **one** suite
  (`suite.rs`) into a module per server, so a claim cannot hold on MySQL and quietly rot on
  PostgreSQL, and a failure reads `mysql::introspection_finds_the_seeded_table` rather than a loop
  that stopped at the first leg. What genuinely differs between servers is read off the leg's
  `Target` (`namespace`) rather than branched on the engine — the same reason production code asks
  a capability instead of comparing a dialect. **MariaDB is a leg of its own**, not a MySQL
  stand-in: the divergences are in exactly what this crate reads, which is how a MySQL 8
  `CHECK_CLAUSE` escaping quirk once hid behind a MariaDB that returned runnable text.
  **The type matrix (`cases.rs`) is what the tier is for.** A value's journey from a column to a
  cell is decided by the driver, the wire protocol and this crate's decoding together, and `core`'s
  tests can only assert what `Value` does with text it is *given*. Each case is a column type, a
  literal and the text the grid must show; each is asserted twice — that it renders, and that the
  same text written back through `export::sql_literal` reads back identically. The second is the one
  that bites, because a lossy rendering *looks* right: reintroducing the historical `bit_cell`
  defect (a bit-field's number wrapped in `Value::Str` rather than `Value::UInt`) leaves the
  rendering matrix green — `"170"` and `170` are the same cell — and fails the write back on both
  MySQL legs with `ERROR 1406: Data too long`, which is exactly how that bug reached a release. A
  case with no expected text (`timestamptz`, whose rendering follows the server's `TimeZone`) still
  asserts the NULL row and the write back; both matrix tests assert a **floor** on how many cases
  ran, since a matrix that walked nothing at all is otherwise a pair of passes.
  **`editable.rs` is the other half nothing else reaches.** Every `edit::analyze_edit` unit test
  hands the ladder a `ColumnOrigin` written out by hand, so what they prove is that it works on
  metadata a test *imagined*; whether a real driver reports `org_table` for an aliased column, a
  `table_oid` for each side of a join, a primary-key flag at all, or anything whatsoever for an
  expression is decided on the wire, and the two halves used to meet only inside the running app.
  `Scratch::edit_model` closes that seam — it runs the query, introspects the scratch database and
  builds the model from both, which is the app's own composition. Each rung of the ladder has a test
  and so does each refusal, and the refusals are the ones that matter: a wrong key does not fail
  loudly, it writes to a row nobody asked for, with only the 1-row net behind it. Removing the
  `NOT NULL` guard from the unique-index rung fails `a_nullable_unique_index_is_no_key_at_all` on all
  three legs and nothing else — checked, not assumed.
  **`writeback.rs` asserts the number the 1-row net reads, where `model.rs` asserts the verdict
  given one.** Two of its claims exist only at this seam. `CLIENT_FOUND_ROWS`: MySQL counts
  *changed* rows by default, so an edit setting a cell to the value it already holds affects 0, the
  net reads that as "the row is gone" and rolls back a perfectly good batch —
  `an_update_to_an_unchanged_value_still_counts_as_one_row` is the only thing anywhere that can
  tell the flag is still set, and clearing it fails exactly the two MySQL legs. And `Rollback::note`:
  the same doomed batch runs twice on the MySQL legs, once on `InnoDB` and once on `MyISAM`, and the
  error has to promise a complete rollback in the first case and admit the surviving rows in the
  second. **`ddl.rs`** carries the designer's round trip — introspect → draft → diff → emit → run →
  introspect — whose real subject is the *asymmetric* failure: an emitter writing something the
  introspector reads back differently leaves a table that is correct on the server and permanently
  dirty in the designer, and neither half's own tests can see it. Breaking `ddl::defaults_equal`
  fails all fifteen of them with `AlterColumn { from: X, to: X }`, which is that symptom exactly.
  **Three capability differences are recorded as leg data rather than discovered per test**, since a
  test asserting one answer would be wrong on two servers out of three: `Target::non_transactional`
  (MySQL's `MyISAM`, which accepts `BEGIN` and ignores it), `Target::transactional_ddl`
  (PostgreSQL wraps a DDL plan, so a refused one applies **nothing** and `DdlError::applied` is 0,
  while MySQL commits each `ALTER` as it runs and the count is what the preview reports), and
  `Target::grants_are_database_scoped` (whether a grant list covers one database, and so whether
  `Grants::note` is there to qualify it).
  **`runtime.rs` covers the four paths that need a connection to *behave*** — `.sql` scripts, bulk
  imports, the pinned manual-transaction `Session`, and cancelling a statement already running — and
  every one of them is an exception to something, which is precisely what a pure test cannot check.
  That `run_script` holds **one** connection for a whole file is asserted by making a temporary
  table in one statement and reading it in the next: under a connection per statement the second
  fails, and so would a dump's opening `SET FOREIGN_KEY_CHECKS = 0`. That a `Session`'s transaction
  is real is asserted from a *second* connection, which must not see the uncommitted row. The
  cancellation test asserts `DbError::Cancelled` **specifically** and that the call returned well
  before the five-second sleep would have ended: asking only "did it fail, and quickly?" is
  satisfied by a sleep statement the server does not understand, which is how that test passed on
  all three legs while cancelling nothing — pointing `Target::sleep_sql` at an undefined function
  now fails it, and did not before.
  **`views.rs` and `triggers.rs`** carry the same round trip for the two objects the engines model
  differently. A view's body is never the text that went in — MySQL fully qualifies and back-quotes
  it, PostgreSQL re-prints it from the parse tree — so the identity diff is doing real work there;
  perturbing `ViewDraft::from_table`'s body by one space fails it on all three legs. A trigger is
  the object they disagree about most (MySQL carries the body, PostgreSQL calls a function with its
  own lifetime), so every trigger test ends by making it fire or proving it no longer does: a
  trigger that sits in the catalogue doing nothing is the failure that reads as success everywhere
  else. Dropping `TriggerDraft::from_info`'s `original` — the same identity-field class as
  `ColumnDraft::original` — fails the trigger round trips on every leg. **A view result is
  read-only on all three**, and that is asserted rather than assumed: the test that meant to check
  it returned early instead and was a no-op on every leg until a probe said so. Its other branch
  still stands, because a driver may reasonably start attributing a view's columns to the table
  underneath, and the key the resolver then picks has to identify exactly one row — which the test
  checks by counting what it matches.
  **`streaming.rs` and `namespaces.rs`** close the last two. The export's assertions are about
  completeness and about how a failure reaches the *writer*: a channel that simply closes reads as
  "the table ended", so a half-written file would be reported as finished — both failure tests check
  the channel, not just the return value. `namespaces.rs` needs two namespaces to exist at all,
  which is what `Scratch::alt_namespace` is for: a schema on PostgreSQL, a second scratch database
  on MySQL, so one test means the same thing on both. What it guards is silent — the statement
  succeeds, one row is affected, the net is satisfied, and the wrong table changed — so it asserts
  on the table that must *not* have moved. Collapsing the schema out of `analyze_edit`'s grouping
  key fails three PostgreSQL tests including that one; the MySQL legs are correctly unmoved, their
  namespace being the database, and that half rests on the same assertion's `database` comparison.
  **In `users.rs` the reads are of accounts that were already there and the writes make their own.**
  An account is server-wide — it is not inside the scratch database and would not go away with it —
  so the read half asks about the account the suite **connected as** (`Target::user()`), that being
  the only one every leg is guaranteed to have, while the write half creates one named with the
  tier's `scratch::PREFIX` and drops it in a guard. That is the same bargain `Scratch` makes with
  databases and it is checked by the same `assert_scratch_name`, called on the way **in and again on
  the way out**, so the rule that nothing here touches what it did not create holds for accounts too
  — and it has to carry more weight here, since an account is not namespaced by anything the server
  enforces. A `ScratchAccount` dropped without `teardown` — a test that panicked — prints the name on
  stderr rather than pretending: `Drop` cannot `await`, and the prefix is what makes the leftover
  self-identifying. What this covers is the half nothing else reaches: three servers, four
  catalogues, and a `mysql.user` column set that differs between MySQL 8 and MariaDB 10 in both
  directions, where a query naming a column the server hasn't got fails outright and only a live
  server says so. Four read tests — the connected account is in the list and carries a host part on
  exactly the engines where an account *is* the pair; its privileges come back as `GRANT` statements
  on both catalogues; the note is present exactly where `grants_are_database_scoped` says and names
  the database it covers; and `no_password_material_survives_the_fetch`, which is the assertion that
  the redaction is on the fetch rather than on one view that happens to call it.
  **The five write tests go through the real emit-and-run path** — `ddl::account` →
  `ChangeSet::emit` → `Db::run_ddl` — rather than asserting statement text, because a statement no
  engine accepts is exactly what only a server can tell you:
  `a_created_account_is_one_the_server_then_lists`, `a_created_role_is_one_the_server_accepts`,
  `a_granted_privilege_comes_back_and_a_revoke_takes_it_off`,
  `a_granted_role_comes_back_and_a_revoke_takes_it_off` and
  `a_dropped_account_is_gone_from_the_list`, fifteen in all across MariaDB 10.11, MySQL 8.4 and
  PostgreSQL 16. The grant round trip reads its
  privilege **off `users::privileges_for`** rather than naming one, and that is the tier earning its
  keep: naming `SELECT` was the first version and PostgreSQL refused the plan, a database being an
  *object* there that carries only `CONNECT`, `CREATE` and `TEMPORARY` rather than a shorthand for
  everything in it. Taking the engine's own first entry is what makes it one test instead of three,
  and it exercises the same list the grant form is built from.
  **The role test reads its role back out of the catalogue before touching it, and that is what makes
  it a test.** It created and dropped one through the principal it had *drafted* —
  `AccountDraft::principal` gives a role no host — so the catalogue's own representation, the one
  every action in the browser acts on, was never exercised, and `core::users`'s MariaDB role-host
  defect sat under a green leg. It now finds the role in `fetch_principals`, reads its grants (the
  statement the browser issues the moment a row is clicked, and the one MariaDB refused with 1141)
  and drops *that* principal through `ScratchAccount::drop_as`, which exists to drop an account as
  some other principal describes it and asserts the two name the same account. Watched red against
  the unfixed fold, with the server's own ERROR 1141 as the failure.
  **What the tier deliberately does not cover: SSH tunnels and TLS.** Not an oversight and not
  difficulty in the tests — the obstacle is that both need a server configured *before* it starts,
  and GitHub Actions brings `services:` containers up before any step runs, so the repository is not
  checked out yet and a generated certificate or key pair does not exist. Covering them in CI means
  baking custom images, committing a test CA and key pair, or dropping those servers out of
  `services:` and starting them with `docker run` inside a step: a CI restructure, decided on its
  own terms. Until then they keep the instrument they have — `db/examples/tls_matrix.rs` over
  `scripts/tls-testbed/`, which reports the transport each mode actually negotiated and is better
  at that than a pass/fail test would be. Also uncovered, for ordinary reasons of nobody having
  needed it yet: streaming a genuinely large export, and multi-schema PostgreSQL.
  **It is gated as a *target*, not at runtime.** The manifest declares the target
  `required-features = ["live-tests"]`, so `cargo test --workspace` does not build it and the pure
  tier stays pure by construction. With the feature on, an unreachable server is a **failure** —
  a harness that noticed a missing endpoint and returned would report a green suite that asserted
  nothing, which is the decoration the testing rules already name. The one exclusion is
  `SCHEMAIC_IT_ENGINES`, which a developer has to type, and which is refused outright when `CI` is
  set: libtest has no runtime skip, so an excluded leg reports as a *pass*, and that is tolerable
  only where a human chose it. Endpoints come from one environment variable per field (no URL to
  parse, no password to encode), defaulting to this project's own test bed.
  **Nothing here touches a database it did not create.** `scratch.rs` generates every name with the
  `schemaic_it_` prefix, the process id and the leg, and both the create and the drop path assert
  the prefix — the drop guard runs during unwinding, where nothing else is checking anything, and
  it deliberately does not panic while panicking (a second panic would abort and take the real
  failure's message with it). CI runs the tier as a blocking `live` job with three service
  containers; the lint job compiles it via `--features schemaic-db/live-tests`, since otherwise it
  would be the one code in the repository no push compiles.
- `schemaic-ai` — persistent `claude` CLI session (stream-json), turn parsing.
- `schemaic-term` — terminal panel + shell (`shell.rs`).
- `schemaic-ui` — the Floem UI. The central `Ui` struct (threaded everywhere) is split per-domain:
  `Copy` signal bundles (`TabsUi`/`SchemaUi`/`ConnUi`/`AiUi`/`TermUi`/`LayoutUi`/`OverlayUi`) +
  `Rc<…Actions>` callback bundles — so `ui.run` is `ui.tab_actions.run`, `ui.db_nodes` is
  `ui.schema.db_nodes`, the tabs signal is `ui.tabs_ui.tabs`. Modules:
  - `consts.rs` — layout/dimension metrics + `MONO_FAMILY` (glob-imported). Any SQL/code
    surface reads that one name — the snippet library's preview, and `FieldCfg::mono` (the DDL
    preview's script box, the view editor's definition). Most of the file is `fn() -> f64` rather than
    `const`, because the interface scale multiplies anything that boxes text; the module doc lists
    what stays a `const` and why — hairlines, editor-relative metrics, seeds for persisted widths,
    icon bases, `TERM_FONT_SIZES`, and the two floating-bar insets — and **that list is the copy to
    extend**, since a prose list has been found short three times.
    `field_input_h()` is the same idea for the
    **compact single-line field** every transient bar wears (the editor's find/replace/goto, the
    grid's find/goto, the row panel's inputs): `FieldCfg::height` is an `Option`, and leaving it
    off is not a neutral default but a *different* control — `None` derives the box from content
    at `line_h + chat_pad_v() * 2 + 3`, which is 34px against this 26. The grid's find bar shipped
    without it and stood 8px taller than the identical editor bar beside it, so a bar that means
    to be compact says so with this metric and never with a literal.
  - `widgets.rs` — reusable widgets: `menu_panel`/`MenuEntry`, `modal_title`/`panel_style`/
    `menu_item_style`, `window_size`, `autohide`/`shift_hscroll`/`wheel_hscroll` scroll wrappers,
    `check_box` — **the app's one checkbox**, drawn by every multi-select list there is (the import
    review list and the dump modal's table picker), so a picker cannot quietly grow a second look;
    it fills and empties **by style, never a rebuild**, which is why its `checked` predicate is
    `Clone` (the fill and the tick each need their own copy inside their own reactive closure); and
    `link_button`, the "Select all" (`accent`) / "None" (`text_muted`) pair that sits above such a
    list — links rather than footer buttons, because they adjust the list the user is reading
    instead of answering the modal, and `in_ring_button`-wrapped because the only other way to
    change a selection is clicking every row, which is not a thing a keyboard can do —
    `section_title`/`centered_msg`/`toggle_icon` (whose `enabled` is **not optional**: every panel
    toggle in the footer needs it, so the ungated shims that passed `|| true` were only hiding the
    fact — see *The footer's panel toggles* below), `tip_when`, `measure_text_px`,
    `jump_to_bottom_button`. Also `sparkle_action` — the sparkle-plus-label "AI fix" the editor's
    error bar and the error modal both offer, in one definition because the two had already drifted
    to different colours by the time there were two of them; **neither half sets a colour**, so the
    row's own tints the SVG's `currentColor` and the words together and one `hover` covers the pair
    (set per child, as the first copy did, the icon stays dark while the label lights up). Also
    `MenuId`/`MenuFlags` —
    the single list of the app's mutually-exclusive dropdowns, which every trigger closes the others
    through (*Popup menus*), and `row_menu_mark`/`row_menu_mark_pad`/`clear_row_mark_on_close` — the
    rule a row wears while its own context menu is open, which lives here rather than in either list
    because the schema tree and Manage Connections must wear the *same* mark and neither owns the
    other (*Popup menus* again, for the 2px the caller has to give back).
    Also `focus_root` — the **one** way an overlay claims the keyboard (see the Floem gotcha on
    directed key dispatch): it takes focus on build *and* registers the view so Escape in a
    text field inside it hands focus back rather than dead-ending. Never spell out
    `.keyboard_navigable().request_focus(|| {})` again; that pair is what left every modal
    unclosable from the keyboard while a field was focused.
    Also the **shared modal form chrome** every modal wears — `form_setting`/`form_section`/
    `form_separator`/`form_gap()`/`control_button`/`footer_button`/`modal_footer`. Manage
    Connections set that shape and Import followed it; a new modal builds on these rather
    than copying them a third time.
    Also the **read-only fact panel** — `fact_section` (a heading with its rows under it), `fact_row`
    (one `label: value` line) and `fact_note` (an icon-led caveat), the three views a panel of
    *observed* facts is built from. They were `properties.rs`'s private helpers until `users_view.rs`
    copied all three character for character, at which point the reasoning existed twice and the
    values could drift apart with nothing on screen to say they had. Both modules call these now and
    keep one-line wrappers over them (`properties::section`/`section_with_gap`/`detail`/`note_line`,
    `users_view::section`/`detail`/`note`) for the label column and row gap that legitimately differ
    between the two panels. The gap and the label width are `fn() -> f64` and not numbers, for the
    reason every other length here is: one resolved at build freezes at the scale it was built at.
    And the **keyboard-navigation cluster**, which is the subject of the Tab gotchas below:
    `FocusRing` (a modal's Tab order — `register`/`unregister`/`step_from`/`remember`/`focus_at`,
    plus the `ring_step` wrap rule and its deliberate opposite `list_step`, which clamps),
    `focus_root_with_ring`/`innermost_ring_root` (how the modal root and the *window* root enter
    it), `in_focus_ring`/`in_focus_ring_with` (how a non-field control joins, the second for one
    with teardown of its own — floem keeps a single cleanup slot), `VALUE_TAB` (where a growing
    block of stops starts), and the `PopupToken`-tagged `set_open_popup`/`clear_open_popup`/
    `dismiss_open_popup` slot. A field joins through `FieldCfg::focus` instead, since nothing
    outside floem's editor can see a key it has.
  - `modals.rs` — **the modal layer** (`modal_layer`) and the four predicates that raise it
    (`ddl_modals_up`, `ddl_editors_up`, `workspace_modals_up`, `settings_modals_up`,
    `modal_backdrop_up`), plus the `modal_backdrop_gate` tests. It mounts no modal of its own and
    paints no scrim: every member view lives in its own module, and this one decides *which box they
    resolve their `inset(0)` against* and *in what order they paint*. Both are policy — the layer's
    box stops at `header_h()` so the title-bar band has somewhere to go, and each paint-order rule in
    the tuple was bought with a bug (a confirm behind Manage Connections, a transaction prompt behind
    the DDL preview, a popup menu behind the panel that opened it).
    **The shared confirm is the layer's last entry, above every group, and that is a rule rather
    than one more ordering.** A confirm is by definition raised *by* something already on screen, so
    "whatever can raise a question comes first" — which `manage_modal` and the DDL preview each state
    about their own neighbour — has exactly one stable answer once the layer holds more than one
    group: put the question above all of them. It used to live inside the DDL group, entry 3 of six,
    while the Live Monitor sits in the workspace group at 5, so *Clear the log?* painted **entirely**
    behind the monitor with its focus root holding the keyboard over a question nobody could see;
    reordering those two groups only moves which modal has the problem. Being its own entry, it needs
    its own term in `modal_backdrop_up` exactly as `find`, `manage` and `plan` do — `ddl_modals_up`
    counted it while it was painted there, and that predicate is now only about that group's own box.
    `modal_up` is a *parameter*
    rather than a local, because `workspace` needs the identical answer for the band and the two
    must not be able to disagree. The group wrappers exist to fit floem's 16-arity `ViewTuple` limit
    and are load-bearing anyway: a group's wrapper fills the layer only while one of its members is
    open, since an always-full-window box would eat every click beneath it. See the absolute-child
    invariant for the full argument and for what the gate holds up.
    **"Open" is not always the same question as "on screen", and `workspace_modals_up` is where the
    two come apart.** The Users and privileges browser renders nothing while one of its own account
    forms or the DDL preview is up — those are raised from it and painted in the *earlier* DDL
    group — so the predicate counts `overlay.users` only while `ddl.account`, `ddl.grant` and
    `ddl.preview` are all `None`. A wrapper that still filled the layer would be a transparent
    full-window box sitting on top of the form, swallowing every click meant for it: the same
    always-full-window failure as above, arrived at through a member that is open but invisible
    rather than through a group that is closed. `users_view::users_overlay` asks the identical three
    signals, and the two must not be able to disagree.
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
  - `settings.rs` — the three settings modals (the main one's groups are General / Editor / Query /
    **Appearance**, the last holding the two theme pickers and the **interface scale**) **and the
    four shared controls every modal's form is
    built from**: `focusable_toggle`/`focusable_toggle_row` (the switch — Space is ours, Enter is
    floem's), `focusable_dropdown` and the picker-agnostic **`in_ring_picker`** under it — the box
    that drops the app's *own* popup menu, which is what every `<select>` in the app is now built
    on (see **No floem `Dropdown`** below). `themed_toggle` is the un-ringed builder beneath, and
    is **private** on purpose: a control nobody can Tab to is one left out of the modal's keyboard
    order by accident.
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
    A right-click in the list on the left **selects on the action taken, not on the click**: its
    menu's Duplicate and Delete carry the connection id themselves, and selecting up front meant
    merely *opening* the menu ran `draft.load` over every field the user had typed, with no undo and
    no copy in the keyring. What answers "which connection is this menu
    about" instead is the shared open-menu rule (`widgets::row_menu_mark`, *Popup menus*), keyed off
    a `menu_row: RwSignal<Option<u64>>` created in the modal's **stable scope** beside `save_flash`
    and `test_flash` and for the same reason — the clearing effect outlives the open/close
    `dyn_container`, and a signal built inside it would be disposed out from under that effect.
    Those two flashes are the transient confirmations standing in for the safe actions' labels (a
    check on **Save**, the result icon on **Test**), and the stable scope was only half of what they
    needed. `save_gen` — the generation the deferred clear checks itself against — stayed inside the
    form the `dyn_container` disposes, so closing the modal inside `SAVE_FLASH` left the pending
    `exec_after` reading `None` from a dead generation: it declined to clear, and the check was
    still on the button the next time the modal opened. What answers it is an effect on `open` that
    **withdraws the confirmation when the modal closes**, rather than hoisting `save_gen` out beside
    the flash — a check reporting a save from a previous visit is wrong even while its two seconds
    are still running (the guard-scope gotcha below states the general rule). **Test has the same
    shape and is deliberately not fixed**: `test_gen` is already in the stable scope so its timer
    does fire, but a close-and-reopen inside `TEST_FLASH` still shows the previous visit's result
    icon, and `test_flash` is also driven by `conn_test` state that would likely need resetting with
    it.
    **`tls_fields` is always visible on a networked engine, not behind a toggle like the SSH
    block**: a database that enforces TLS is the ordinary case rather than the advanced one, and a
    checkbox marked "use SSL" is the control that leaves people believing a connection is verified
    when it is only encrypted — so it is a named level with `SslMode::description` printed
    underneath. What it shows below the picker comes from the mode's **capabilities**, never the
    variant: the CA path appears when `verifies_certificate`, the client identity when
    `negotiates_tls`, so a sixth level would not need this view found and edited. An *empty* CA
    path under a verifying mode means **whatever the operating system trusts** (`db/tls.rs`), and
    the hint says so, because a blank required-looking field otherwise reads as unfinished.
    Every TLS signal joins host/port/user/password
    in the effect that **invalidates a prior Test result**, since raising the mode or naming the
    wrong CA turns a working endpoint into a refused one and a green Test left standing over that
    is the most misleading state the indicator has. `path_field` is the shared path+Browse control
    behind the SSH key and all three certificate paths; folding them together also fixed the
    original, whose picker sat two Tab stops from the field it fills in while its own comment
    claimed otherwise.
  - `connection_import.rs` — the **Import Connections** modal, over `core::conn_import` and the
    app's `conn_sources`. Raised from the Manage Connections list ("Import from another client",
    below New connection and quieter than it: it is the first-run action, and the one nobody
    presses twice), and painted *directly above* that modal in `modals.rs` — the question is about
    the list behind it, so closing returns to it. Its own term in `modal_backdrop_up`, like
    `manage_open`'s, because it is a loose child of the layer and can outlive the modal that raised
    it.
    **Three ways in, and the modal opens empty.** A pasted *Connection URL* at the top, then
    *Choose a file…* and *Scan installed clients* beneath it, then — only once one of them has
    produced something — the review list. Opening does **not** scan: the walk reads the user's home
    directory, and a dialog that goes through it because it was opened is doing something nobody
    asked for. The three sources all append through the app's one `add_import_rows`, so a scan
    cannot discard a URL pasted before it and the three cannot disagree about ticking or about
    duplicates. `empty_message` (pure, tested) is why the scan button doesn't look dead on a
    machine with none of those clients: an empty list means three different things — an invitation,
    progress, an answer — and `ConnImportUi::scanned` is the bool that tells the first from the
    third.
    A review list, not a form: a row is a *proposal* with a tick box, its name, where it points
    (`row_target`, mono — `user@host:port/db`, or the file's name on SQLite; pure and tested, and
    deliberately **not** a `scheme://` URL, since this project has no URL builder on purpose and
    one written to fill a label is what the next reader copies for something that matters), and
    capsules for the engine and the source. Every `ImportNote` it carries is joined into a third
    line shown only when there is one — a row can be both already-saved and password-less, and
    showing only the first hides the one that decides whether to tick it. Rows that repeat a saved
    connection arrive unticked. The tick box is `widgets::check_box` — the app's one checkbox, which
    was written here first and moved to `widgets` once the dump picker adopted it — and it changes
    **by style, never a rebuild**; the
    selection is a `HashSet` of indices so a row's read is O(1) — the two rules `dump_view`'s table
    picker paid for. Indices are safe because `rows` only ever grows at the end or is reset
    wholesale by `open_import`.
    The footer is rebuilt on **whether anything is selected**, not on how many — an Import button
    left enabled over an empty selection imports nothing while looking like it worked, but keying
    the container on the *count* rebuilt both buttons (and their focus-ring registrations) on every
    tick of a selection still being made. The count is needed for the label alone, so it goes
    through `widgets::action_button_dyn`, whose label is reactive while the button around it is
    not. The "Added N connections." line shows only while nothing is selected, which is exactly the
    window between an import finishing and the user asking for another — so it can never sit beside
    an enabled Import describing a *previous* press. `skipped_sentence` (pure, tested) names up to
    three left-out entries and counts the rest, returning `None` for an empty list so a stray
    "0 entries were not imported" can't reach the screen.
  - `dividers.rs` — the two **panel** dividers: `h_resize_handle` (the schema tree's and the right
    panel's edges) and `v_resize_handle` (the editor/results split), plus the `DelayedHover` they
    share. Not `window_chrome::resize_zones`, which resizes the *window* and is mounted outside the
    app root. Both handles are absolute children positioned from an **effective** (clamped or
    floored) edge rather than from the dimension they set: a width the window is too narrow to
    honour, or a height persisted under a lower floor, would otherwise leave the handle floating
    away from the edge it drags. Both capture the pointer on press (`request_active`), and **both
    undo the whole gesture inside `on_double_click_stop`** — the double-click eats the second
    `PointerUp`, so that handler is the only one that runs and anything the `PointerUp` handler
    would have cleared has to be cleared there. Two things qualify, found a year apart in the same
    four lines: the drag state (`dragging` + `clear_active`, or the handle stays captured and keeps
    resizing on mouse-move with no button down), and the **hover highlight**. The second is the
    subtler one — `dim.set(default)` moves the handle out from under a pointer that has *not* moved,
    so floem delivers no `PointerLeave` and nothing else ever turns the bar off: the divider
    animated to its default still lit, and stayed lit there until the next mouse move happened to
    trigger a leave. `hovered.leave()` also voids a pending arm, so a double-click *inside* the
    hover delay cannot light the bar afterwards. `dividers::double_click_gate` asserts both calls
    are present in **every** double-click handler in the file — the two handles are twins, so every
    fix here is two edits, and it was watched failing on each of them in turn.
    **The dividers light up on a *rest*, not on a pass.** The bar is an affordance — this edge
    drags — and the dividers run the full height and width of the workspace, so answering on
    `PointerEnter` lit one every time the pointer crossed from the schema tree to the editor or from
    the editor to the results. `RESIZE_HOVER_DELAY` (200ms) is how long the pointer must settle
    first — 500 was tried and is too long: it outlasts the gesture, so a pointer that has already
    stopped on the divider reads as one the app has not noticed. Dragging is not delayed and the hit
    band is never gated on the highlight: the delay is on the hint, never on the control. There is
    no cancelling a floem timer, so the arm carries a **sequence number** and checks it with
    `try_get_untracked` when it fires — one comparison that retires both the pointer having left
    (`Some(newer)`) and the divider having been disposed inside the delay (`None`, since
    `exec_after` timers outlive the scope that armed them). That second half is defensive and **not
    currently reachable**: `v_resize_handle` is called once from `center`, itself built once in the
    workspace shell, and the per-tab `dyn_container`s are its siblings rather than its parents, so
    these signals live as long as the app does.
  - `history_panel.rs` — Query History right-column panel; its row preview is syntax-coloured on
    `theme::preview_bg` like the snippet library's, for the `contrast.rs` reason `snippet_panel.rs`
    gives below. Its **dialect comes from the active connection**, same memo and same reasoning as the
    library's: the list is filtered to that connection, so every row on screen ran against it. That
    memo is tracked in the row `dyn_container`'s trigger tuple alongside `visible` and `search` —
    two connections that both have no history yield the same empty `visible`, and without it the
    rows would stay built with the previous engine's lexer.
    **A row's right-click menu takes the snippet library's route exactly** —
    `menus.close_except(Some(MenuId::Popup))`, `popup_anchor` cleared so the panel opens at the
    pointer, `popup_width` from `menu_panel_width(&entries)`, then the fill — and the width is
    *measured* rather than a constant for the reason recorded there: `popup_width` is the panel's
    `min_width`, so a number picked by eye becomes a floor the rows cannot pull back in. `row_menu`
    offers **Open in new tab** (what the single click already does) and **Delete**, with a
    `MenuEntry::Separator` before the destructive one, the same convention the library's row menu
    follows. **Delete is deliberately not behind the shared confirm the trash button uses**: that
    one clears a whole connection's log at once and names the count in the modal, while this
    destroys the one row under the pointer and re-running the statement records it again — a modal
    would cost more than the mistake it prevents. The opener is built **once in `history_panel`**
    as an `Rc<dyn Fn(HistoryEntry)>` that `history_row` takes as a single parameter, rather than
    assembled per row: `overlay` and `menus` are panel-wide `Copy` bundles, threading them through
    every row would say otherwise, and the row goes on taking only what it draws with.
  - `snippet_edit.rs` — the snippet editor modal: **the one place a saved query's body can be
    changed**, with its name and abbrev alongside. The panel's inline fields cover the two
    one-word edits because those are the same act as renaming a tab; a body is SQL, multi-line, and
    a 300px panel row is the wrong shape for it, so it gets the modal chrome the view/trigger
    editors wear (`modal_title_owned` / `modal_footer` / `panel_style`) and the same multi-line
    mono field with `tab_indents`. **Save writes only the fields that changed, through the same
    per-field actions the inline edits use** — there is deliberately no fourth "save everything"
    action, because two paths writing one field is how the two drift. It is painted inside the
    `ddl_modals_up` group and is therefore *in* that predicate: an overlay in that group whose flag
    the predicate doesn't know about resolves its `inset(0)` against a zero-by-zero box and paints
    nothing, which is exactly how the event editor once shipped invisible.
  - `source_gate.rs` — **test-only**, and the shared machinery behind the crate's *source gates*:
    the tests that read the crate's own `.rs` files and fail on a spelling production code must not
    contain (a floem `Dropdown`, a captured `Color`, a raw pixel inset, an unguarded `exec_after`,
    a menu trigger that doesn't close its siblings). `production_code` strips every `#[cfg(test)]`
    **item** — brace-aware, skipping braces inside strings, chars and comments — and every `//`
    line; `crate_sources` enumerates the files to scan. Both halves exist because the idiom was
    written out eleven times across nine files and every copy had the same two holes. It cut each
    file at the **first** `#[cfg(test)]`, which is right only for a file whose tests are all at the
    bottom: `widgets.rs` has an inline test-only `fn` a tenth of the way in, so its gates read 929
    of 7,259 lines and a planted `Dropdown` at line 2000 passed. And it walked
    `env!("CARGO_MANIFEST_DIR")/src` — `schemaic-ui` alone — while the invariants the gates enforce
    are stated app-wide, so a violation added to `schemaic-app` (which builds views too, in
    `app_view`) passed the whole suite. A copy each also meant that fixing either hole reached one
    gate of eleven.
    Two details in there are load-bearing and read as tidiable. **Not every `#[cfg(test)]` sits on a
    block**: it can be on a `use` (ends at `;`), on a struct field, an enum variant or a match arm
    (ends at the `,`, or at the enclosing `}` when it is the last member), so `item_end` reading only
    `{`/`}` made that last case decrement past zero and panic with *attempt to subtract with
    overflow* — taking all eleven gates, and so the whole suite, down with a message naming neither
    the file nor the construct. And `(` and `[` are counted alongside `{` for one reason: without
    them a `,` at "depth 0" lands inside `fn f(a: u32, b: u32)` and hands the body of a test-only
    function back as production code. An unbalanced file is refused rather than guessed at — the rest
    is scanned, because a false positive fails loudly and a silent truncation is the failure this
    module exists to end. `crate_sources` closes the same hole at the other end with
    `assert!(out.len() > 20)`: a moved or renamed `src` would otherwise pass every gate by finding
    no files at all.
  - `snippet_panel.rs` — the **Snippet Library** right-column panel (`RightPanel::Snippets`, the
    toolbar's bookmark toggle): the saved queries that apply to the active connection, under the
    scope bands `core::snippet::grouped` returns, over the History panel's chrome. It decides
    nothing — which snippets apply, how they group and sort, and what the filter matches are all
    `core::snippet`'s answers, under test. Its **dialect comes from the active connection**, not
    the active tab: a tab keeps the connection it was opened on, while the library sits under the
    connection selected above it. **A row click inserts the body at the caret** (`Tab::insert_req`),
    where a history row opens a new tab — a record of a past run wants its own tab, a snippet is
    something you compose *with*; "Open in new tab" is on the row's menu for when it isn't. The
    parameter chips under a body are read live from `params::names`, never stored, so they cannot
    disagree with the body above them. **The body preview is syntax-coloured** (`highlight_sql_mono`,
    over the editor's own `sql_highlight::highlight_spans`, with the search match added last so it
    still wins the overlap) and sits on **the editor's own surface, not the panel's** — the token
    colours reproduce published palettes tuned against the *editor* background, which is the pairing
    `contrast.rs` gates and the only one it vouches for, and the editor theme is chosen
    independently of the light/dark UI theme (Catppuccin Latte's dark-on-light tokens can be live
    while the panel is dark). That surface is `theme::preview_bg` and the uncoloured base beside it
    `theme::preview_fg`, both named there rather than spelled here so the cross-axis gate measures
    what the panel paints. It was `bg_editor` — the UI theme's text-field surface, chosen off the
    *other* axis — which is the pairing `contrast.rs`'s entry records at 1.70:1. The Query History
    panel's preview runs through the same `highlight_sql_mono` onto the same two colours (a long
    list of past runs read as the same wall of grey this one did); and the AI panel's fenced blocks
    take the same two accessors, a **third instance rather than the exception** they were once
    recorded as here — a SQL block goes through the same `highlight_sql_mono` once the turn has
    settled, and a block that stays plain (non-SQL, or still streaming) keeps `preview_bg` and
    `preview_fg` regardless, a second background for it being two kinds of block in one
    conversation. The two previews are the same treatment
    down to the base the uncoloured identifiers take: the panels share a column and are read the
    same way, and a brighter base in one of them spent on identifiers the contrast the keywords are
    there to carry.
    **A saved snippet is scoped to the connection it was saved on and stamped as just-used**, so it
    lands in the topmost band's topmost row and the panel scrolls there — the first spelling (engine
    scope, no stamp) dropped a new row into the middle of a long alphabetical list, where nobody
    could find the thing they had just saved. Widening it afterwards is one click in *Show in*. The
    scroll goes through a `scroll_to` signal **cleared on the next tick**: a `Some` left standing
    re-scrolls on every later layout pass, which is the sticky-`scroll_to` trap that also shaped the
    grid's tail-follow.
    Naming that new snippet is inline (an `edit_field` in place of the name), matching the tab
    rename, because there is no text-prompt modal here and naming a saved query is the same kind of
    act. That is the **only** inline edit: the name and the abbrev both used to have menu entries
    and inline fields of their own, which made three routes into the three fields `snippet_edit`
    already owns — the menu now offers *Edit* and the dialog does the rest. The menu's **Show in**
    submenu is the scope picker: its three choices are `snippet::scope_options` (narrowest first), so the order you pick
    from and the order of the bands a row can move to cannot drift apart — a test pins each choice
    to the band it lands under. The current one is tinted rather than ticked, the convention
    `cell_editors::pick_entries` set. A built-in offers none of the three: Duplicate is how you get
    an editable copy of one.
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
    *app*, not here, so no route to a kill can skip it. **The lock-wait banner's Kill is the one that
    answers the keyboard**, through `widgets::key_pressable` — navigable, Enter and Space press it —
    because it is built by hand rather than through `widgets::action_button`, whose family is the
    modal-footer one and needs a `FocusRing` this panel has none of. It carries the id it will kill in
    its label: a button reading just *Kill* on a panel where several sessions are killable is the one
    misread that cannot be undone. The **per-row** Kill and Cancel hang off a right-click menu with no
    keyboard opener and are still pointer-only, which is why `README.md`'s accessibility sentence was
    narrowed to claim only what is true. The clock in the title bar wears the same grey as
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
  - `users_view.rs` — the **Users and privileges** browser (`users_overlay`), over `core::users`.
    It lists the server's accounts, shows one account's privileges, and is where the four write
    actions are raised from — the forms themselves are `account_editor.rs`. Mounted in the
    workspace group beside `properties_overlay`/`erd_overlay`/`monitor_overlay` and counted by
    `workspace_modals_up`.
    **It renders nothing while one of its own forms or the DDL preview is up.** Its `dyn_container`
    is keyed on `(target, hidden)`, where `hidden` reads `ddl.account`/`ddl.grant`/`ddl.preview`,
    and the outer fill style asks the same question. That is the pairing every schema editor already
    has with the preview one level up, and here it is load-bearing rather than tidy: the two account
    forms live in the modal layer's **DDL group**, which is painted *before* the workspace group, so
    both opened *behind* the browser that raised them. `modals::workspace_modals_up` asks the
    identical three signals, because a wrapper that still filled the layer would be a transparent
    full-window box sitting on top of the form and swallowing every click meant for it. Cancel over
    there returns here with the list intact.
    **Opened from the SCHEMA gear and from a right-click on the tree's blank space, for the reason
    `Create database` sits in both**: an account belongs to the *server*, not to any row in the tree,
    so the modal has no object and takes none, and the gear is not a duplicate of the blank-space
    entry because a connection whose tree already fills the panel leaves no blank space to
    right-click. In the gear it sits **above** `Create database` — a read entry before a write one,
    skeleton group 2 before group 4. It shipped below it, which is the cross-group inversion
    `menu_order_gate` exists to catch and cannot see in this menu, so the gear now reads Refresh ·
    Collapse all · Users and privileges · Create database · Show table sizes. The entry
    is **absent** where the engine has no accounts at all (`users::supports_users` — SQLite) and with
    no saved connection (`ctx.exists`), and **dimmed on a down connection but not on a read-only
    one**: browsing accounts writes nothing, so the read-only refusal that guards `Create database`
    would be answering a question this action does not ask, while a connection that cannot be reached
    would open the browser onto a fetch that fails. `open_for_server` is the one door, and it resets
    every signal it reads on the way in rather than on close, so a second opening cannot flash the
    previous server's accounts while the new list is in flight; the database it is given is the
    active tab's, because that is the one PostgreSQL's schema and table privileges can be read from.
    Two panes: a filtered account list (`users::matches`, one field over the whole `app@host` display
    name) on the left, and the selected account's attributes and `GRANT` statements on the right.
    **The list column runs the full height of the body and the footer belongs to the right column**,
    which is Manage Connections' shape and was adopted from it: a footer spanning both put a rule
    through the list just above its last row, and the count under a list the count is not about. The
    column keeps its `border_right`, so the only line crossing it is the one dividing the two panes.
    Inside it the search box is full width (placeholder `Search accounts`), the list sits 10px below it
    with 5px between rows — Manage Connections' figure, two lists of names in one app spaced the
    same — and `+ New account` is an `in_ring_button` row pinned at the foot: `CIRCLE_PLUS` plus accent
    text at `menu_item_style`'s 12px inset, the shape and the place `New connection` has.
    **The list is keyed on `(state, filter)` and deliberately not on the selection** — picking a row
    would otherwise throw the whole list away to change one row's background, and take the scroll
    position under the pointer with it — so the selected row's background is a *reactive style* over
    `overlay.users_selected` instead. That closure reads it with **`with`, never `get`**: it runs for
    every row on every restyle, hover included, and `get` clones the whole `Principal` and its
    attribute vector to answer one equality test — the per-item case the Floem gotcha below is
    about. **The rows wear the connection list's affordances, copied
    rather than approximated**: resting `theme::conn_list_text()`, hover brightening the *text* to
    `conn_list_sel_text()` with no background behind it, and selected being that same bright text on
    a full-width `conn_list_sel_bg()`. A row is full width and carries the 12px inset as its own
    padding — the column carries none — because otherwise a selected background stops short of the
    column's edges; the search box repeats the inset for the same reason. A `system` account stays
    `text_faint` in **every** state, and that colour is set on the label rather than on the row, so
    the row's own colour cannot overrule it. Copy is always enabled and a no-op before a selection,
    the same bargain the properties modal's Copy makes: the alternative is a button whose enabled state is a
    `dyn_container` over the fetch, rebuilding its focus-ring registration on every click. It copies
    what the pane shows, which is already redacted, because redaction happens at the fetch.
    **Each `GRANT` is rendered with `widgets::highlight_sql_mono`** — `theme::preview_fg` on a
    `theme::preview_bg` block, the same call Query History, the snippet library and the AI chat's
    fenced SQL make, with the dialect read off **`UsersTarget::dialect`** — captured at open beside
    `conn_id` and for its reason, since the statements being highlighted belong to the server the
    browser was opened on rather than to whichever connection the switcher has since moved to. Its
    two sibling targets (`AccountTarget`, `GrantTarget`) carry one for the same reason. It was a
    plain monospace `text()` on `code_bg`. The
    two preview colours are the **editor's** axis and are paired deliberately — `contrast.rs`
    measures them against each other — so the base has to be `preview_fg` and the surface has to be
    named `preview_bg`, even though that accessor resolves to `code_bg` today: a coloured block
    taking `preview_fg` onto anything the cross-axis gate does not read would be untested for
    legibility. And a `GRANT` now reads the same here as in the tab it would be pasted into.
    **Each write action sits beside the thing it acts on**, not in the footer: `+ New account` at the
    foot of the list column, under the list it adds to rather than beside the box that searches it,
    so the column reads top to bottom as *find one, or make one*; and a `Privileges` /
    `Drop` pair under the selected account's name. That pair is an `Option<AnyView>` the detail pane
    **extends its section list with**, never an `empty()` placeholder: the stack has a 16px gap and
    floem gaps an empty child like any other, so an absent actions row left a hole between the
    account's name and its attributes — the trap `properties::stats_body` states. The footer's
    actions are about the *modal* —
    copy what is shown, close it — and a Drop down there would sit one Tab from Close, which is the
    wrong pair of neighbours for an irreversible action. `WriteGate::of` is asked **once, in
    `users_overlay`**, and the answer is passed into both panes: the two rows asked it independently
    to begin with, which is two places for one answer to drift — one of them leaving `+ New account`
    live while `Privileges`/`Drop` were dimmed, with nothing on screen to say which was right — and
    it re-walked the connection list for an answer that cannot differ between them. Its capability
    half reads `target.dialect` and its read-only half the *live* connection, deliberately: what the
    browser is about cannot change while it is open, and a read-only setting can. It gives
    **three refusals with three remedies**, because they have three different answers: no
    engine support is *absent* (there is nothing about this connection the user could change),
    while read-only and no-database-selected are both dimmed and each say under the buttons which
    one it is. The last is the reason the sentence is there at all: an account change takes the
    ordinary in-database route (`ddl::is_account_change`), so with nothing selected there is nowhere
    to send it, and a dimmed pair with no explanation reads as a bug. Actions are
    **never offered for an account the server maintains** (`Principal::system`, the flag that
    already dims its row): dropping `mysql.sys` or `pg_monitor` breaks the server rather than the
    account, and no privilege screen should make that one click away — the pane says so in a line
    instead. Drop goes through a `Confirm` whose body is the change's own `Change::risks` via
    `overlays::risk_prompt`, so the question and the preview's warning cannot drift into saying
    different things about one act, and then through the preview like everything else.
    **Reversing a listed `GRANT` into a `REVOKE` by parsing it was built, tested and then removed.**
    It is the feature the right-hand pane invites — a button beside each statement — and it needed
    either a raw-statement `Change` variant, which would have become the escape hatch every
    structured change routes around, or a parser whose wrong guess emits a `REVOKE` that takes away
    more than the line the user clicked. The grant form serves both directions instead, which is
    what its Action toggle is for.
    - The two requests are tracked apart — `UsersState` for the list, `GrantsState` for one
      account's privileges — for the reason the properties modal's exact `COUNT(*)` is tracked apart
      from its statistics: a failure to read one account's grants must not replace a list of accounts
      that loaded fine. `SchemaActions::principals` asks `supports_users` before the round trip and
      reports `UsersState::Unsupported`, which is a different thing to say than a fetch that failed
      and is distinct from an empty `Loaded` (that would read as "a server with no users").
    - Both callbacks outlive a close, so each checks `overlay.users` still holds the target it was
      asked about; `grants` additionally checks `users_selected` is still the same account, since a
      second click while one is in flight would otherwise land the wrong account's privileges in the
      pane. Nothing is persisted — the browser opts out of `SavedTab` by construction, as every
      other modal does.
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
    would re-read the file per keystroke and stamp over a hand-edited mapping.
    **Excel adds an `xlsx_settings` block beside `csv_settings`**, built and torn down rather than
    hidden, for that section's Tab-order reason: a Sheet picker and the "First row is a header"
    toggle, which is all a workbook has to be asked (there is no delimiter and no quote). The header
    toggle is repeated rather than lifted out of `csv_settings` and shared, since hoisting the one
    setting the two formats have in common would move a control CSV users already know the position
    of. The picker is `settings::in_ring_picker` **directly** and not `focusable_dropdown`, because
    that wrapper needs `T: Copy` and a sheet name is a `String`, and it is built only when the
    workbook has two sheets or more — a dropdown with one entry is furniture. **The picker sits in
    its own `dyn_container`, keyed on `sheets`, inside the block's** — which is keyed only on
    whether the format is Excel. The list is rewritten on *every* probe, and floem's `dyn_container`
    takes no `PartialEq` (`create_updater` does not dedup), so keying the outer block on it rebuilt
    the header toggle the instant the user pressed Space on it, dropping keyboard focus. `ImportUi`
    carries the two signals behind it, `sheets` and `sheet`, cleared in `open_import`, again when a
    probe reports a *different* file, and again when a probe **fails**: a sheet list left standing
    would describe the previous workbook beside an error about this one, and clearing the chosen
    sheet is also what unwedges a name carried over from a file that had it. `sheet` joins the
    settings effect above, because which sheet is read is a fact about how the file parses exactly as
    the delimiter is. The probe itself fills `ImportProbeResult::sheets` from the same parse as the
    sample and **skips the CSV sniffer for a workbook**: an `.xlsx`'s head is deflated ZIP bytes, so
    running the sniffer over it would let a compressed stream's byte frequencies decide `has_header`.
    While a load runs,
    the footer's Cancel fires `SchemaActions::import_cancel` (the app owns the token, as it does
    for query runs) instead of closing — the transaction rolls back, so a cancelled import writes
    nothing.
  - `script_view.rs` — the **script-load** modal, **Import** to the user, over `core::script`; the
    inverse of `dump_view.rs` below, and the entry directly *above* it in the two menus that carry
    both (a database's and a PostgreSQL namespace's), where the group reads
    **`Import → Export → Create ▸`**. Those menus could write a `.sql` file nothing here could read
    back, which is the hole this closes. **Import leads and the order is not alphabetical**: it is
    the entry that changes the database, so it takes the slot furthest from where the pointer rests
    after a right-click — the reasoning that keeps `Drop` last everywhere else in these menus.
    **One word, two scopes.** *Import* on a **table** is `import_view`'s CSV/JSON/Excel loader;
    *Import* on a **database** is this. A script has no table to load into because its statements
    name their own, so the scope of the node picks the loader — and the namespace a namespace-node open carries is
    for the **title only**, since nothing can confine someone else's `CREATE TABLE` to `sales`.
    **Its own `ScriptUi` bundle rather than an arm of `ImportUi`**, though the user reaches both
    through one word: a CSV import is twenty-odd signals about delimiters, headers, null tokens and a
    column mapping, and a script reads none of them. Folding it in would have meant branching every
    one of those on a scope that leaves them all unread; sharing the entry point and the frame is the
    part the user can see, and is what was actually asked for.
    The panel is `dump_view`'s shape because the two are one journey in opposite directions — pick a
    file, see what it holds, run it with progress and a stop, read the outcome — and it shares that
    modal's rules: it **stays open while the run goes** (its signals are the run's only channel, so
    closing would hide work in flight), every exit routes through one `exit_action` so the footer,
    Escape and the ✕ cannot disagree, and the dismissive slot wears **Stop** in `Danger` while
    running. The second step is `core::script::probe`'s readout — the kind histogram, what the file
    destroys, and whether it opens its own transaction — with every count printed through
    `count_label`, so a bounded probe says `400+` rather than a total it did not earn.
    **Run is `ActionKind::Primary`** — the same button the table import's own *Import* is. It was
    `Danger` for one build, on the argument that this runs someone else's DDL with no undo; what
    that actually produced was one modal reachable from two menu entries wearing two different
    confirming colours, which reads as two features rather than as a warning. What the file will do
    is said in words, in the panel, where it can be specific about *this* file. `open_script` clears
    `ui.dump.target` and `open_dump` clears `ui.script.target`: the two share one tuple element in
    the modal layer and each fills it when open, so both being set would stack two full-screen
    overlays — a rule the trigger/routine/event group next to them also states, and one that relying
    on reachability alone has already broken once.
    The guard is `sql::script_verdict`, asked in the same synchronous step that launches
    (`widgets::accept_launch`) — see the write-guard invariant for why it is stricter than
    `run_verdict` rather than a second, laxer gate.
  - `dump_view.rs` — the **schema + data dump** modal, **Export** to the user, over `core::dump`.
    Three entry points and one modal: a database's schema context menu and a PostgreSQL namespace's,
    where it is the middle of **`Import → Export → Create ▸`** — the three entries about the node as
    a whole, behind their own separator — and a **table's, directly below *Import***. All three pair
    it with the import it is the round trip of. It is the one label that sits inside a writing group
    without writing anything, which is why `menu_order_gate` exempts it from the skeleton by name:
    it writes a *file* and never the server, so it can never be the irreversible entry the ordering
    exists to place. The table
    entry passes `open_dump`'s `preselect`, which ticks that one table instead of all of them, and
    deliberately passes **no namespace**: the picker still lists the whole database, because
    narrowing to the table's own namespace would hide the neighbours its foreign keys point at. It is
    offered on a **read-only** connection, unlike Import — it reads the server and writes a local
    file. The code keeps the `dump` name throughout, for the reason `core::dump`'s entry gives. It
    collects a table selection and the six `DumpOptions` and hands both to
    `SchemaActions::dump_run`; nothing here builds SQL. The picker is **its own read**
    (`SchemaActions::dump_tables` → `Db::fetch_table_list`, names only — `fetch_schema` would read
    every column of every table to print them), because it has to be right for a database the tree
    has never been expanded on, where a picker built from the cached tree would silently offer
    nothing; a namespace's entry then filters that list to its own (`dump::tables_in_namespace`), so
    a `sales` dump carries no `public` table. That filter goes through `schema::sql_qualifier` and
    **not** a bare `"{ns}."` prefix, because `display_name` *omits* `public` — matching on the prefix
    filtered a `public` dump down to nothing, none of its tables carrying one, and `None` from the
    qualifier means "the unqualified ones are mine", which is exactly the set to keep. What opens
    ticked is `dump::initial_selection`: everything, the common case being all of it — or exactly the
    preselected table when the entry was a table's own, since the click already said which one, and
    only if the read's list actually contains that name — when it doesn't, the modal **says which
    table is missing** in its error line. A click that names a table the server no longer reports
    (dropped or renamed since the tree was last refreshed) otherwise opened a full list with nothing
    ticked and a dead `Export` button, which reads as broken rather than as an answer.
    The picker's rows read the selection through a `create_memo` of it as a `HashSet` and draw
    `widgets::check_box`, the app's one checkbox, which fills and empties by style (`s.hide()` on
    the tick), never a per-row `dyn_container`. It wore a check-glyph-and-hollow-square pair of its
    own until the import list's box was made shared: two spellings of "ticked" in two modals is the
    inconsistency a shared widget exists to prevent. Its header went the same way — the count, then
    `widgets::link_button`'s "Select all" / "None", where two `ActionKind::Quiet` buttons labelled
    "All" / "None" used to be. `chosen` stays a `Vec`,
    because that is what the request carries and what "Select all" resets from, but every row
    watches it, so
    a single tick cost a linear `contains` per row *plus* a rebuild of every row — quadratic in the
    number of tables listed. The memo makes each row's read O(1) and the show-hide is the *Floem 0.2
    gotchas* rule (`display: none`/`flex` beats a rebuild), which here also means nothing is taken
    apart under the pointer mid-click.
    It follows `import_view`'s discipline for the same reasons: `widgets::accept_launch` in the same
    synchronous step as the launch — inside the save dialog's callback (titled `Export to SQL`,
    defaulting to `{database}.sql`), since that is where the launch is, and with `read_only` false
    because a dump writes to the local disk and never to the server — a `listing`/`running` pair
    that is what the buttons gate on, and `DumpUi::generation` bumped on every open so a table list
    or an outcome that lands after the modal was reopened elsewhere reports into nothing. Every exit
    — the footer's dismissive button, Escape, the ✕ — **stops the export rather than closing**
    (`widgets::exit_action` with `cancellable: true`), the import modal's rule and for its reason:
    closing would hide a write still going and leave its outcome with no reader, since the modal's
    signals are the only channel the run reports to.
    **The footer carries the run; the body is choices only.** The dismissive button is where the
    stop lives: it reads `Close` and turns into a red `Stop` (`ActionKind::Danger`) for as long as
    that is what pressing it does — a word and a colour, because in `Neutral` it reads as the same
    way out with a different label, which is what it is not. `Export` is disabled while the run is
    on. **The stop is deliberately not beside the progress text**: it was, for one revision, and the
    line grows as its count does (`3 of 12, 9k` → `98k rows so far`), so the one control the user
    wanted walked left and right under the cursor on every tick. The footer's **left** slot
    (`modal_footer_split`) therefore carries text alone — the per-table line,
    `Writing orders — 3 of 12, 56k rows so far`, the count through `text::human_count`, the same
    abbreviation the grid uses — falling back to an animated `Reading the schema`
    (`widgets::loading_dots`) before the first table, where there is nothing to count yet and a
    static label and a hung one look identical. The **outcome** replaces it in that slot: the green
    `Wrote 5 tables.` followed by `export::export_note` — the grid's own caveat wording, reused
    rather than restated, so a file whose every blob went out as `NULL` says so instead of reading as
    a clean success — and the red failure or cancel sentence. A ticked table the run's **own** fresh
    introspection could not find (`DumpPlan::missing`) is named in that same green sentence rather
    than only in the file's header, for the reason the tally is named there: a file one table short
    of what was ticked looks exactly like a complete one. `DumpUi::done` is therefore the
    finished `String` rather than the counts for the view to word: it is built in the outcome
    callback, which is the last place the destination's file name is still in scope, and
    `export_note` names that file. Both used to sit at the
    bottom of the modal's scrolling body, where the thing whose whole purpose is being seen could
    land below the fold; nothing about the run is written there now. The progress line is its
    **own** `dyn_container` inside the footer's, because it ticks once per table and rebuilding the
    buttons beside it on each tick would take the focus ring with it. The whole footer is a `dyn_container` keyed on
    running / the chosen set / `DumpUi::options().is_empty()` / the outcome, the shape the import
    footer uses and for the same reason: `action_button` takes a plain `bool`, so a state read while
    the panel is built is the state at build time, and an `Export` button left enabled after the last
    table is unchecked launches an export of nothing.
    It is painted in the DDL group, so `modals::ddl_modals_up` has to name `ui.dump.target` — the
    wrapper's `inset(0)` otherwise resolves against a box that predicate is keeping at zero by zero,
    and the modal renders nothing at all. The foreign-key toggle is offered only where
    `dump::fk_guard_sql` returns something, and where it doesn't the modal *says* so in a `form_hint`
    rather than greying out a control: PostgreSQL's switch is superuser-only, so the honest thing is
    a sentence about the constraints section, not a checkbox that fails the restore for most roles.
    `watch_connection` closes the modal when the active connection changes under it — the Export
    button would otherwise launch against a `conn_id` that is no longer selected — unless a dump is
    running.
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
    **`ddl_preview::close_peers` is the one editor-target list**, called by every editor's `open`
    before it sets its own and by `close_editors` after an Apply. Each of those `open`s used to
    keep its own copy and they had **drifted**: the table designer cleared the view editor and
    nothing else, the view editor the designer and nothing else, while the object, routine and
    trigger editors cleared four apiece — every one of them carrying the same comment about two
    panels being painted at once, which is precisely what a partial list does. Six flags maintained
    by hand in five places is where one list becomes the only version that stays true. The single
    exception is the caller's, not the list's: `keep_trigger` is what leaves a half-filled trigger
    form standing under the routine editor it opened.
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
    `is_editable_view` is the entry point's gate — a materialized view reaches no editor, and
    the two things its context menu can still do to it are **Drop** and **Refresh view**
    (`ObjectEntries::refresh_view`, over `ddl::refresh_view_change`), neither of which opens a
    form. Its materialized half is `ddl::is_materialized_view`, the same predicate the menu
    asks to *offer* the refresh: two hand-written copies are two chances for the editor and
    the menu to disagree about one node.
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
    `over_backdrop` answers the same class of problem — **a modal used to take the
    window frame with it.** Every backdrop covers the whole window, the title bar included, so with
    a modal up the window could not be dragged, minimized, maximized or closed until it was
    dismissed — the resize edges were the only part of the frame that stayed reachable, precisely
    because they are mounted outside the root. Its two views are **not** out there with them,
    though, and the difference is exact: a resize zone has to be hit before the whole app, while
    the band only has to out-paint *the header and the modal layer*. Mounted at the window root it
    was also above the overlay menus — and a menu tall enough to take `menu_inset`'s
    "bigger than the window" arm pins at y=0, so its first rows sat under this scrim and answered a
    press with an OS window drag instead of with the row the pointer was on (hoisted submenus had
    the same exposure). It lives inside the root's tuple now, after the modal layer and before
    `date_pick_overlay` — which are precisely the three overlays that can open from *inside* a
    modal. `over_backdrop` is one draggable strip across the
    bar, raised only while `modal_backdrop_up` says a backdrop is on screen, carrying the scrim
    colour itself (the modal layer no longer reaches the header) and stopping short of the caption
    buttons at `controls_width` so the *header's own* buttons stay the live ones — one set of
    close handlers, and the strip it leaves clear is exactly the strip that still works. It is
    deliberately not a copy of the header: a press anywhere along it moves the window, because
    nothing else up there does anything while a modal is up and a real title bar drags from
    anywhere. It stays **under** the resize zones, which are the layer outside the root, so the top
    corners still resize rather than drag. `controls_width` is the one place `control_w()` meets `Chrome::own_control_count`, and a
    source-scanning test pins that against the buttons `controls` actually builds — a fourth
    caption button added without touching the count would leave 46px of title bar dimmed and dead.
    It returns **two** siblings, not one, for the reason the zones are eight: the header's
    `border_bottom` (`theme::HEADER_BORDER`, inside the 40px box) is one rule across the whole
    width, and the band stopping short of the buttons left it dimmed up to them and lit for the
    last 138px. The second view dims that sliver, and is `pointer_events(false)` — paint only,
    since it lies across the bottom edge of all three buttons and a 1px sibling on top of a control
    still ends the walk. That flag's usual objection (it takes the subtree with it) costs nothing
    on a view with no children.
    **A press on the chrome hands the keyboard back**, deferred, after a title-bar drag and after a
    caption press (`give_the_keyboard_back` → `widgets::hand_keyboard_back(None)`). Floem clears
    focus at the top of *every* `PointerDown` dispatch and only a `keyboard_navigable` view re-takes
    it during the walk that follows; the band and the caption buttons are neither navigable nor
    inside anything that is, so a press on them left focus at `None`. Inside the app that is
    invisible, since the window root's own listeners still see the key — over a **modal** it is not,
    because the modal's `focus_root` requests focus once on build and nothing re-requests it, so the
    panel went keyboard-dead: its ✕ and Cancel still worked, Tab recovered through the root's ring
    backstop, and **Escape has no equivalent backstop**, so the one keyboard route out of the modal
    was gone until the panel was clicked. `None` is the argument because that call already resolves
    the innermost mounted focus root and falls back to the workspace's keyboard home, so the chrome
    needs to know neither. Deferred because the clear happens *before* the listeners run, so a
    request inside the same dispatch would be undone by it. **Close is excluded** — it is the one
    press after which there is no window to hand a keyboard back to.
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
  - `event_editor.rs` — the **scheduled event** modal, over `core::ddl`'s `EventDraft`, on the
    one engine that has them. Reached from the schema tree's Events folder (row **Edit**),
    from the database and namespace **Create ▸ Event** submenus, and from Find-Anywhere.
    Modelled directly on `routine_editor` — the same chrome, the same
    seed-local-signals-then-write-back rule, the same `DdlUi::event_body` arrangement for the
    field a late `SHOW CREATE` has to correct in place, the same `overlay_open_key` memo so that
    correction doesn't tear the modal down, and the same `ddl_preview` ending. What differs are
    four consequences of `ALTER EVENT` being in-place: there is no drop-and-create, so a rename
    can't destroy the original and the footer's clash message says only that the name is taken;
    the three Preview refusals (`event_source_pending`, `event_body_stale`, `name_clash`) cost a
    *rejected statement* rather than a lost object, and are still made because an error after
    Apply is a worse way to learn this than a footer; **the schedule is the one part of the form
    that rebuilds itself**, keyed on the shape (`EVERY` vs `AT`) alone and never on the draft, with
    the draft written *before* the flag is flipped so the rebuilt fields seed from the shape being
    switched to; and every timestamp field holds SQL, so it is monospaced and its placeholder
    shows the quotes. The Status dropdown offers Enabled and Disabled plus, for an event already in
    it, the replica state — offering `DISABLE ON SLAVE` freely would be offering a keyword MySQL
    8.4 has removed, while hiding it would show "Enabled" over an event that isn't.
    **A dropdown that offers "the standard list, plus whatever this event already uses" has to read
    the second half from what the *server* reported**, not from the draft: `status_choices` and
    `interval_units` both take `ui.ddl.event`'s `target.current` alongside the draft value, because
    the form is torn down and rebuilt whenever the preview opens or closes. Seeded from the draft
    alone, moving a `SLAVESIDE_DISABLED` event to Enabled, editing its body, pressing Preview and
    coming back left the list reading `Enabled / Disabled` — "Disabled on replica" gone, no other
    producer of `EventStatus` in the view, and the only way back to the state the event is actually
    in being Cancel and reopen, which throws away every edit made in the same sitting since `open`
    replaces the draft wholesale. `interval_units` is fixed with it rather than left as the one found
    next time (`EVENT_INTERVAL_UNITS` is exactly MySQL's documented fifteen, so nothing is reachable
    through it today), and it compares case-insensitively — the server's spelling is its own, and a
    duplicate differing only in case is a list with two identical-looking rows.
    **That state has two spellings and `EventStatus` keeps them apart** (`SlavesideDisabled` /
    `ReplicaDisabled`), because the word the server reported in
    `information_schema.EVENTS.STATUS` is the only signal for which keyword it will accept back:
    8.4 renamed the column value *and* removed the old keyword. Folded together — as they were —
    every statement used the pre-8.4 spelling, so Copy CREATE on 8.4 produced SQL it rejects and a
    whole-database script stopped at that event. Two variants rather than one carrying a word, so
    that equality still means "the same state" and the differ can't see an `ALTER` in an event
    nobody edited.
    It shares one tuple element with the trigger and routine overlays (Floem's 16-arity
    `ViewTuple` limit) and clears both of their targets on open, as they clear its.
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
    `ObjectDraft::from_item`/`blank` return `None` for a routine **and for an event**, and this
    module's own `open_for_object`/`open_for_new` are what route those to `routine_editor` and
    `event_editor` — so the tree, the palette and the menu all keep asking one function to open an
    object, and a routine handed to a type form would have to be an explicit mistake rather than
    whichever arm compiles.
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
  - `database_editor.rs` — the **container** form: a database, or one of PostgreSQL's
    namespaces inside one, over `core::ddl`'s `DatabaseDraft`. The smallest of the schema
    editors and the only one that **only ever creates** — there is no `current`, no diff and
    no change count, because a container is dropped from its own row's menu and neither engine
    offers a rename that is safe to perform (MySQL withdrew `RENAME DATABASE` in 5.1.23;
    PostgreSQL's needs every session off the database). The footer therefore says what will be
    made rather than counting changes, and the panel takes its height from its content, since
    it is two to four rows tall depending on the engine.
    **Three homes.** The schema tree's **Create ▸ Database / Schema**, the SCHEMA gear's
    **Create database**, and a right-click on the tree's **empty space** — which is the one
    people reach for first, since every file manager trains it. The gear is not a duplicate of
    that third: a connection whose tree already fills the panel has no blank space left to
    right-click, which is exactly the connection on which a new database is worth making.
    `create_children` and `schema_tree::blank_space_entries` are the two places that decide
    which entries exist, each with its own test. **The read-only refusal is inside
    `open_for_new` itself**, not at the four launch sites: the invariant is that a launch guards
    itself in the same step that launches it, and written per site that is four copies of one
    `if` — of which `create_submenu`'s arm had already been left out, relying on
    `MenuEntry::disabled` alone. One refusal at the door is the only version that survives a
    fifth home. The entries stay dimmed, because that is what *says* the action is unavailable.
    The option fields are per-engine and **absent** rather than dimmed where the engine has
    none — asked as `ddl::supports_owners` / `supports_database_charset` rather than as
    `dialect ==` tests, which is what both were written as first. MySQL gets `Character set` /
    `Collation`, PostgreSQL gets `Owner`, and PostgreSQL's `ENCODING` is deliberately not
    offered at all: the server refuses it unless the locale clauses and `TEMPLATE template0`
    agree, so a field for it would mostly produce *"new encoding is incompatible with the
    encoding of the template database"*.
    All three are **free text with a `suggest_chevron` beside them**, the designer's
    column-type pattern, because every value they take is per-server and per-version — a
    collation only MariaDB has, a role created five minutes ago. The MySQL lists are the
    constants `ddl::MYSQL_CHARSETS`/`MYSQL_COLLATIONS`; the collations are deliberately **not**
    filtered by the chosen character set, since filtering would make it a picker that has to be
    *right*. Owner's list is `Db::roles` (`left(rolname, 3) <> 'pg_'`, **not** a `NOT LIKE
    'pg\_%'` whose escape only survives while `standard_conforming_strings` is on), fetched on
    open like the trigger editor's functions — which is why `suggest_chevron` takes its options
    as a **closure read at press time**: the reply lands after the form is built, and rebuilding
    the row to deliver it would tear down a field the user may be typing in. That closure is also
    why the chevron now **refuses to open on an empty list**: `popup_menu_overlay` builds a panel
    for any `Some`, and an empty entry vec still draws — height collapsing to the padding and
    border, width clamping to the 170px floor — so a press before the fetch landed painted an
    empty bordered box that then ate the click meant to dismiss it. Unreachable while every
    caller passed a constant.
    **The footer says nothing while the form is valid**, unlike every peer modal. Theirs show a
    change count, which is a diff the reader cannot otherwise see; here there is no diff, so
    the only sentence available restated the title and the name field — and a status line that
    only ever agrees with the screen teaches the eye to skip the place errors appear.
    `change_of` is the pure mapping from kind to `Change`, and it is unit-tested because a
    swapped arm is invisible in a rendered form: the two kinds are different statements at
    **different levels** — `CreateDatabase` is server-level and `CreateSchema` is not.
    `ddl_preview::preview_container` is the one exit, and it reads that level off the change
    (`ddl::is_server_level`) rather than taking a caller's word for it.
  - `account_editor.rs` — the Users and privileges browser's **write half**: two overlays in one
    module, `account_editor_overlay` (create an account) and `grant_editor_overlay` (grant or
    revoke), both raised from `users_view` and both ending at `ddl_preview::preview_account`, which
    is the third sibling of `preview_change`/`preview_container` and the only thing here that runs
    anything. Mounted in the modal layer's DDL group sharing `object_editor.rs`'s tuple element,
    counted by `ddl_editors_up`, and on `PAINTS_A_BACKDROP` as one file with two overlays. The
    read-only refusal is inside `open_for_new`/`open_for_grant` rather than at the button, the same
    rule `database_editor::open_for_new` follows: a launch guards itself in the step that launches
    it, and the browser's dimming is what *says* the action is unavailable.
    The account form **only ever creates** — the shape `database_editor` has and for the same
    reasons: an account is dropped from its own row in the browser, and neither engine offers a
    rename that is safe to perform. Its Kind picker comes first because it decides what the rest of
    the form means: a role takes no host and no password on either engine, so those fields **vanish
    rather than sitting there inert**, and Host is absent on PostgreSQL, which has no such thing at
    all. **The form holds a password, and nothing else in this crate does.** It is blanked on every
    open — a form that reopened holding the last one would put a credential on screen nobody typed
    this time — cleared on Cancel, never persisted and never logged, and it becomes visible in
    exactly one place: the preview's SQL. That is deliberate. The preview is the app's one gate
    between a plan and a server, and a statement shown there with a field blanked out would not be
    the statement it ran.
    **Every fixed-list choice in both forms is the app's `<select>`** — `settings::focusable_dropdown`,
    the control the settings modals wear, so the popup, the keyboard, the tinted current value and
    the chevron box are one implementation. Four rows moved onto it: the account form's **Kind**, and
    the grant form's **Action**, **Subject** and **Level**, all of which were rows of `action_button`s
    that read as picked or not, and the module's own `toggle_button` is gone with them.
    `bound_dropdown` and `bound_toggle` sit beside `bound_field` and exist for the reason it does:
    `focusable_dropdown`/`focusable_toggle` bind to an `RwSignal<T>` and these values live in a
    **field of a draft struct**, not in a signal of their own, so each seeds a local signal once and
    writes back through an effect only on a genuine change — a rebuild cannot read as an edit.
    `bound_dropdown` takes `label` as a `fn` rather than a closure because `focusable_dropdown`'s is,
    and wraps the control at `field_w()` so a form of mixed controls lines up.
    `picked_outline(style, picked)` survives the move with **one** caller left, `privilege_tag`:
    `theme::accent()` when chosen and `Color::TRANSPARENT` when not, so only the colour changes.
    Taffy sizes the border box, so a 1px rule added *only* while picked grows a button sized by its
    own padding by 2px, which is what the Kind, Level, privilege and option toggles all did the
    moment they were clicked while they were buttons — and in a wrapping cloud it re-flows every tag
    after it on the line. It is the same accounting `widgets::row_menu_mark_pad` does one level down,
    taken from the other end: a tree row's height is fixed by what surrounds it so the 2px is given
    back out of its padding, while a button here has nothing to give back, so the border has to be
    there all along. See the UI convention.
    The grant form is **one form for four statements** — grant or revoke, privileges or a role —
    and the mapping from its Action and Subject dropdowns to `Change::{Grant,Revoke}{Privileges,Role}`
    is `core::ddl::grant_change`, pure and unit-tested rather than four arms in the render.
    **The Action dropdown is over a `bool`**: `GrantDraft::revoke` stays the `bool` that
    `PrivilegeChange` and `ddl::grant_change` read and that their tests pin, and the free
    `action_label(bool) -> &'static str` gives it its two words. An enum invented for the form would
    be a second spelling of the same fact sitting one conversion away from the tested one.
    It opens pre-picked to the widest level the engine has, so the name fields mean something before
    the user has noticed the picker, and **changing the level clears the ticked privileges**: kept,
    they would carry `EVENT` down to a table level that has no such privilege and emit a statement
    the server refuses. That pre-picking is `initial_grant_draft(dialect)`, its own function beside
    the openers rather than a literal inside `open_for_grant`, because the **Level row exists only
    when the draft holds a level** — `if let Some(current) = seed.level`, and no row at all when it
    is `None`, since a dropdown with nothing in it is a worse answer than no row. **The row has no
    fallback of its own, deliberately**, and that absence is the first thing a future editor would
    tidy back in: an `or_else(|| levels.first().copied())` here would paint a level the draft does
    not hold, while the fields below stay gated on `seed.level` and so stay hidden — and because
    `bound_dropdown` writes back only on a *genuine change*, picking the very entry already shown
    would not be the change that unstuck it. With the seeding in one named function, `None` means
    exactly *an engine with no levels*, which cannot reach this form at all
    (`users::supports_user_admin` gates the browser's button). The coupling is pinned by
    `the_grant_form_opens_holding_a_level_wherever_the_engine_has_one` and
    `the_level_it_opens_on_is_the_first_the_picker_offers`.
    **The privileges are a wrapping tag cloud** — `h_stack_from_iter` under `FlexWrap::Wrap` at 6px,
    where they were one per line. Eighteen is a legal selection at MySQL's database level, and
    eighteen rows is a column of short words taller than the panel: a set you have to scroll to see
    the shape of. Wrapped, the whole set is one block, which is the question the row is actually
    asking — *which of these*. `privilege_tag` (once `privilege_row`) still reads the draft inside
    its own style, so clicking one tag does not rebuild the cloud of eighteen, and it is the one
    control here still wearing `picked_outline`.
    The database the browser is scoped to is *suggested* beside the qualifier
    field rather than filled in, since a prefilled name on a form that grants privileges is a value
    nobody read.
    **Both forms' `dyn_container`s are keyed on a memo over the form's *shape*, never on the draft
    signal** — `account_form_shape` (the Kind) and `grant_form_shape` (`(subject, revoke, level)`),
    two pure functions naming the fields that decide which rows exist, because the values in those
    rows do not. Keyed off the draft, both rebuilt the entire form on every keystroke in a name
    field and on every privilege tag, tearing the field down mid-word and taking the caret with it:
    floem's `create_updater` does **no equality check**, so a key that merely recomputes to the same
    value still fires. That is `widgets::overlay_open_key`'s bug, reproduced in two more places.
    `form_shape_tests` pins the two functions — typing or tagging never changes a shape, each
    dropdown does — and **that is the whole of what it can see**: whether the key closure reads
    `shape.get()` or the draft is a line in a view, which is how `overlay_open_key`'s own pin, taken
    on the memo in isolation, let the regression walk past it. Preview SQL is gated on
    `users::GrantDraft::is_ready`, the same predicate the footer's sentence is written from, and the
    press then asks `ddl::grant_change`, which answers `None` for exactly the drafts that predicate
    rejects — the doubled refusal every launch here makes, since a disabled button is not a guard.
    **"With grant option" and "With admin option" are the app's switch** — `settings::focusable_toggle`
    through `bound_toggle`, reading the draft, so a yes/no in this form reads as a yes/no everywhere
    else one appears. They shipped as an always-"Yes" button whose `current` parameter was ignored,
    so the two rows that exist only to show a state showed none of it: nothing on screen said whether
    the statement about to be previewed would carry the clause.
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
    menu, generic grid popup, the date picker's calendar (`date_pick_overlay`, whose panel is
    `cell_editors::calendar_panel` — up here because the field it drops from sits inside a scrolling
    strip that would clip it), Find-Anywhere, error modal. **What separates the two kinds here is
    the backdrop, not the name.** The menus are shrink-wrapped to their panel; Find-Anywhere and
    the error/confirm/transaction prompts paint `modal_backdrop()` over the window, which puts
    them in `modals::modal_layer` with the rest of the modals. Find-Anywhere is the one that
    looks like it belongs on the other side of that line — it is a palette, and a click away
    closes it — and it is the reason the line is drawn where it is: mounted with the menus, its
    backdrop covered the title bar, and a press aimed at the caption buttons found the click-away
    instead, so the only thing the window did was dismiss the palette. Its `find_top()` is measured
    from the top of the *window*, so the layer's `header_h()` comes off it where the margin is set —
    and it is **scaled, because what it is subtracted from is**: as a literal `80` against a
    `header_h()` that grows, the gap under the title bar ran 48 / 40 / 28 / 16px across the four
    scales, shrinking as everything around it grew, which is the inversion no reader of the number
    would predict. `menu_icon_tuck()` and `menu_edge_pad()` are functions for the same reason — an
    offset frozen between two growing boxes is not an offset but a drift, and at 160% a 154px panel
    tucked by a literal 30 landed its right edge ~18px inside the icon instead of flush past it.
    **`risk_prompt(change, dialect)` is where a destructive confirm gets its body**, from the
    change's own `Change::risks` rather than from a sentence typed at the call site, so the question
    and the preview's warning cannot say different things about one act. It was
    `container_drop_prompt` while the database and schema drops here were its only callers; it was
    always generic over any `Change`, and the account drop `users_view` raises is what made the
    narrower name wrong. An empty `risks()` — an arm a later edit emptied — falls back to a question
    rather than to a modal with a blank body, which is an irreversible action asked with nothing in
    it (`a_riskless_change_still_asks_something`).
  - `schema_tree.rs` — SCHEMA sidebar (`schema_panel` + db/table/column/key row builders + keyboard
    nav).
    **A right-click on the tree's empty space raises its own menu** — `Users and privileges`,
    `Refresh` and `Create database`, all three about the *panel* rather than about anything in it,
    because nothing in it was clicked. It hangs off the tree's own box with no hit test of its own:
    every row raises its
    menu with `on_secondary_click_stop`, so what reaches this handler is exactly a click that
    landed on no row, and a hit test here would be a second answer to a question floem's
    propagation already answers. It uses the **generic** `popup_menu` channel rather than
    `context_menu`, which carries a `CtxKind` describing the row that was clicked — and this
    menu exists precisely because there wasn't one. `blank_space_entries` is the decision, split
    out to be asserted like `overlays::create_children`, and it returns `BlankEntry`s carrying a
    `BlankKind` rather than bare labels, because the builder routes on the discriminant: on labels
    with a catch-all arm a third entry falls into whichever branch was written last and renders as
    a live row that does nothing.
    **The early return now asks whether the engine has anything here beyond `Refresh`** —
    `databases || users`, where it used to ask `supports_database_editing` alone. SQLite still
    raises **nothing at all**, having neither a database to create nor an account to browse, which
    is the original point: a one-row menu on blank space reads as a misfire, and the gear still
    carries Refresh for anyone looking for it. Left asking only the first question, that lone
    `Refresh` would have appeared the moment a second entry arrived.
    `Users and privileges` is skeleton group 2 and sits **after** `Refresh` rather than before it,
    where the skeleton's wording says that group closes with `Refresh`. That is a deviation of the
    same kind as the two recorded at `group` for the context menus, and tolerated for the same
    reason — it stays *inside* the group, so the cross-group rule is untouched and the read entry is
    still above the write one. What moves is only which of the two reads is under the cursor, and
    `Refresh` is both what this menu is opened for most often and the only entry in it that costs
    nothing. It is dimmed on a **down** connection and **not** on a read-only one —
    browsing accounts writes nothing, and the write actions inside the browser gate themselves —
    where `Create database` is dimmed on either. `blank_space_is_a_subsequence_of_the_skeleton` is
    this menu's own half of `overlays::menu_order_gate`, which cannot see a menu built in this file;
    see that gate for what it does and does not reach. The tests look entries up **by label**
    (`labels`/`disabled`, the second panicking when the row is absent) rather than by index, so a
    fourth entry cannot silently shift what an older assertion checks, and an assertion about a row
    that is not there fails instead of passing by finding nothing.
    The standalone objects hang off the same levels the tables do, in
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
      edge is the one that moves. **Vertically it centres rather than anchors** —
      `align_self(AlignItems::Center)`, not `inset_top(0)` — and the two are not interchangeable:
      taffy places a start-anchored absolute child at the border box *plus* the container's border
      on that side (`flexbox.rs`, `perform_absolute_layout_on_absolute_children`: `start +
      constants.border.cross_start + margin`), so the 1px rule `menu_mark` adds while a row's
      context menu is open pushed the badge down by exactly 1px, and right-clicking a table
      visibly nudged its size until the menu closed. `height_full()` still resolved against the
      border box, so the box was displaced rather than squeezed, and the chevron, icon and name —
      flex children, which that border does not move — stayed put, which is what made it read as
      the badge's own bug. The centring branch instead derives the cross offset from
      `content_box_inset` (padding + border) at *both* edges, so a rule present above and below
      cancels. The horizontal anchor was left alone on purpose: `inset_left` measures from the
      border box the same way, but a row wears no left or right border, so there is nothing there
      to displace it. The trade is that a table name long enough to reach the panel
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
    over `schemaic_core::intel`'s scope/context engine. Its inputs travel as one `CompletionCtx`
    (catalogue + dialect + snippet library + connection): they are only meaningful together, and a
    call pairing one connection's schema with another's snippets is a bug no longer signature could
    catch. **Snippet abbrevs are a suggestion tier**, above the keyword continuations and skipped
    after a `qualifier.` — an abbrev is a name its owner chose *in order to type it*, so a match on
    one is not the guess a ranked keyword is. One row per distinct spelling, each resolved through
    `snippet::by_abbrev` so the narrowest scope wins by the same rule everywhere; the row shows the
    abbrev and inserts the **body**, through the same `Suggestion::insert` override an FK-JOIN row
    uses.
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
    Italic also costs no width, so unlike `tab_dot_w()` and `tab_file_w()` — neither of which is inside
    `tab_title_avail()`'s 40, so a title has to shed whichever of them is showing or a full-width one
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
  - `cell_editors.rs` — the **type-aware value controls** the grid edits with when a column's legal
    values are already written down: the picker (a box, or the in-cell face), the `SET`'s chips and
    the calendar. Every builder binds to the same `RwSignal<String>` a text field would have, so the
    staging, NULL and write-back machinery around it is unchanged — a control is only ever a
    different way of typing the same string, and *which* string is [`core::celledit`]'s to say
    (including what each row **writes**, which is not what it reads: a boolean's row reads `true`
    and writes `1`, a `SET`'s writes the whole value with that member toggled). Three shapes, each
    avoiding a specific failure: **one picker for booleans and enums alike**, because a boolean's
    values are as listed as an enum's and one control for both is one thing to learn — a two-row
    menu rather than a switch or a checkbox, since the value has a third state ("nothing chosen
    yet") neither has anywhere to stand for. It opens the app's own `menu_panel` rather than a floem
    `Dropdown` because of the paint-only edge nudge documented in `settings::scale_picker`, and the
    row panel sits at the window bottom where that bug lands. The **`SET`** gets chips in the row
    panel because a menu closes on the first pick and a subset is picked repeatedly (in a cell,
    where there is no room for chips, it uses the picker and toggles one member per opening). The
    **calendar** is a panel in the window's own overlay layer (`DatePick` →
    `overlays::date_pick_overlay`), placed by the pure `calendar_insets` against a `calendar_size`
    that is *computed, not measured* — which is what makes its edge flips exact rather than
    estimated. **Every length in that computation is the same function the panel's own style calls**
    — `calendar_pad`, `calendar_gap`, `calendar_band_h`, `calendar_headings_h`, `day_w`/`day_h` and
    `CALENDAR_BORDER` — because both were literals in two places and a scale sweep moved only one of
    them: the style call scaled and the measurement did not, so above Normal the panel was *declared
    smaller than its own contents*, which does not clip but overflows the background and the border,
    and makes the edge flip an under-estimate at the same time. The border is the one term that stays
    a `const`, being a hairline at every scale — the same distinction `menu_panel_height` draws, and
    `calendar_size` had it backwards, scaling its boxes and treating its air as the hairline. Its
    `DatePick::anchor` carries **both** vertical edges of the control, unlike
    `PopupAnchor::BelowBox`'s three numbers, because it flips through `widgets::box_menu_inset`:
    down from the control's bottom, up from its **top**. The row panel is a strip at the bottom of
    the results area, so the flip is the common case there, and a panel measured from the bottom
    edge (`menu_inset`'s cursor rule) opened straight across the button — which is also the button
    that closes it. `open_calendar` is the one place a `DatePick` is built, and `toggle_calendar` is the
    row panel's second-press-closes wrapper over it; a grid cell opens the panel directly, one tick
    after its field is built. `DatePick::on_pick` is what the two openers do *not* share: it runs
    when a day or **Now** has been written and only then, so the row panel (whose field is what
    commits) leaves it `None` while the cell editor stages its edit there. Every path that writes the
    buffer inside `calendar_panel` leaves through the same `done` closure, which is what keeps
    "closed" and "chose something" from drifting into one answer. `widgets::open_picker` is the one
    menu opener — it lives there, not here, because every `<select>` in the app is one of these
    pickers now (see **No floem `Dropdown`**); it anchors at the control's own
    `ViewId::layout_rect` (already in window coordinates), so a menu reached by Tab doesn't open
    wherever the pointer was left, recognises its own menu to toggle it closed, and closes every
    other menu — which it can because `PopupChannel` carries the whole `MenuFlags` rather than the
    single channel it fills. A trigger that swallows its own press owes both halves of that bargain
    (see *the app's menus are mutually exclusive*), and the calendar's toggle owes it too. **Which
    calendar belongs to which field is settled by the buffer, not the anchor**: one channel serves
    every date field in the panel, so asking only "is a calendar open" lit every field's button at
    once and let any one field's teardown close another's panel. That identity is what the grid's
    `cell_calendar_up` asks too — a cell editor always binds `gs.edit_buf`, which no row-panel field
    ever is. **The calendar peels its own Escape** (`peeling_escape`, wrapping the row panel's
    `on_escape`): every other control's popup takes the keyboard, so the key reaches the window root
    and `dismiss_open_popup` answers it — but a day, a month arrow and *Now* are not focusable, so
    the field beside the calendar keeps the focus and stops the key before the root ever runs.
    Escape closed the row panel out from under an open calendar. While it is up the toggle is lit,
    and **it pins its own hover**: it is the calendar's dismissal then, and `field_box`'s hover tint
    would otherwise repaint the accent border grey under the pointer.
  - `grid.rs` — the whole results grid (`GridState`/`GridCtx`; `loaded_view` is the entry point,
    called from the results strip's body). `editor_pane.rs` — SQL editor pane
    (`query_pane` + the Ctrl+K bar, statement highlight, custom scrollbars). `compute_diagnostics`
    bridges the tab's schema/active-db to `intel::diagnostics`; `syntax_view` draws severity-coloured
    squiggles (red errors / amber typo warnings) with hover tooltips.
    **The Ctrl+K suggestion is not drawn here** — it is phantom rows in the editor's own line flow
    (`inline_diff`, below), and what `cmdk_popup` still draws is the two ends of the block: the
    question bar (sparkle, a `multiline` `edit_field` capped at three rows, then a send affordance or
    the `verb_spinner`) anchored under the statement, and the verdict footer — Accept / Reject / the
    change count, read off the *published* plan so there is
    no second diff of the same text — anchored under the added rows. Neither covers the editor any
    more, so the compact↔expanded animation went with the box it was growing.
    **The overlay's `dyn_container` is keyed on a `Memo` of `(open, is_ready)` — not on the state,
    and not on a bare closure**, and both halves of that are load-bearing. Keyed on the state, every
    Idle → Busy → Failed transition rebuilt the question field, and a freshly built `edit_field` has
    almost no width for one frame, so a one-line question wrapped in that frame and the field latched
    the row count it measured there. The bar reopened a row taller with its text stranded at the top,
    which looked exactly like a typed newline and sent three rounds of fixes at the Enter key (see
    `edit_field`'s Enter contract for the misdiagnosis). Computing the pair in the key closure did
    **not** fix it, because `dyn_container` does not diff — it swaps the child on every *run* of the
    key, whatever the value (`create_updater` → `swap_val`), the same trap the Live Monitor's body
    documents. Only the memo, which notifies solely when the value differs, actually spares the
    field; a fourth round of Enter fixes went to the wrong place for want of that distinction.
    Everything that differs across those states is therefore reactive *inside* the branch: the
    trailing slot (send triangle vs `verb_spinner`) is a nested `dyn_container`, the failure message
    is another, and `on_submit` reads the state when pressed instead of being rebuilt per state.
    **That trailing slot reserves the spinner's width while the triangle is showing** — its verb is
    picked once per opening and the slot takes `loading_dots_w` of it, `justify_end` so the triangle
    keeps the right edge. The field beside it flex-grows into what is left, so without the
    reservation submitting swapped a 15px icon for up to 102px of "Metamorphosing..." and took that
    width off the question mid-flight — a second, quieter way to the same re-wrap.
    A sent question is **dimmed and takes no more typing**, through `FieldCfg::frozen` — a `Memo`
    the *built* field reads, not a second spelling of `read_only`. That distinction is the whole
    point: `read_only` and the text colour are resolved when the field is built, so following the
    request through them meant rebuilding it, which is the re-wrap above. Both were dropped for a
    while on the reasoning that editing mid-flight is harmless (the request carries the text it was
    sent with) — but an editable box promises an edit that cannot reach the question already asked,
    and the spinner alone was left saying so. `frozen` drives the editor's own `read_only` signal
    through an effect (floem re-reads it per keystroke in `TextDocument::receive_char` /
    `run_command`, so there is no second gate to fall out of step with it) and dims the field's own
    colour to half inside its reactive style. **The focus effect has to honour it too**: an AI fix
    opens the bar already `Busy`, and focus arrives after the freeze, so a focus that reset the
    caret put a blinking one in a field that takes no keys. The input
    row also carries **no vertical padding of its own**: `edit_field` already puts `chat_pad_v` above
    and below its text, and the second helping made a one-line question sit in a bar deep enough to
    read as two.
    An **effect** over
    `inline_ai` publishes and clears the preview rather than a set inside each transition: it maps
    `Busy` to an `InlineView::Working` over `diff::line_span` of the captured range and `Ready` to a
    `Plan`, and *everything else to `None`* — every way out of `Ready` (approve, reject, Escape, a
    second Ctrl+K, a tab switch cancelling the generation) has to take the rows down, and those
    share no single caller. The buffer read sits in a closure **only those two arms call**: Idle and
    Failed are the common transitions, and they were copying a whole 190 KB script out of the rope to
    compute offsets they then discarded.
    **With nothing selected, Ctrl+K widens to `sql::statement_range`**, the same "what does this key
    act on" answer Ctrl+Enter gives; it used to capture a bare caret, so an unselected Ctrl+K asked
    the model to edit an insertion point and left the diff nothing to replace. It reads the text for
    that from **`e.doc().text()`, not the `query` signal** — the offsets it is resolving are the
    document's own, and `accept` re-reads the document for the same reason. The right-click *Ask
    AI* entry widens, selects and anchors identically: it is the same action reached another way,
    so it has to pick the same thing. Ctrl+K then **selects** that range for real
    (`cursor.set_insert`), which is where the design's selection colour behind the statement comes
    from: the honest way to show what the key picked is the editor's own selection rather than a
    lookalike overlay, and it leaves the user able to see, extend or replace the range with the
    gestures they already have. The bar is anchored at the **end** of the acted-on range rather than
    at the caret, so it sits under the whole statement instead of splitting one in two, and every
    entry point goes through `anchor_cmdk` to do it.
    **That sentence was not true of *Optimize*, and `cmdk_open_gate` is what makes it a fact rather
    than an intention.** Four entry points open the bar — the key, *Ask AI*, *Optimize*, and
    `fix_with_ai`'s three menus — each written separately, and *Optimize* selected nothing and
    anchored nothing. The two symptoms did not look like one bug. With no selection, the in-flight
    fade over the acted-on lines was the only sign of what was about to be rewritten, so the gesture
    that reads as "this statement" everywhere else read as *the editor going dim*. With no anchor,
    the bar opened at whatever `cmdk.point` the previous Ctrl+K had left — the origin on a fresh
    tab, so it drew **over** the statement, and pressing Ctrl+K once anywhere and closing it again
    "fixed" it for the rest of the tab's life, which is the tell for a stale anchor and reads as
    nothing else. It also dropped its `highlight_pick`: the statement border belongs to the action
    whose answer lands *elsewhere* (*Explain*, which sends prose to the chat panel and needs
    something in the editor pointing back), while an action that rewrites the statement in place has
    the selection as its marker. The gate is a source gate for the reason the crate's others are —
    the thing under test is a set of call sites — and it bounds its look-back to a window rather
    than latching a flag per file, since a 5.8k-line file has unrelated `set_insert`s in it.
    `anchor_cmdk` does two things neither caller should
    repeat. It stores the point in the editor's **content** coordinates and leaves the style closure
    to subtract the viewport, so the bar tracks a later scroll — while the closure was *not*
    subtracting it, an open in a scrolled editor placed the bar by how far down the document the
    statement is rather than by where it is on screen, which put it at the bottom of the pane. And it
    **scrolls the editor when the bar would not fit** below the line: the bar is an overlay, so the
    editor does not know it exists and will leave the anchor line flush against the bottom of the
    pane, opening the prompt clipped or entirely out of sight. `CMDK_BAR_RESERVE` is the room it asks
    for, deliberately generous — over-scrolling by a few pixels is invisible, and
    `scroll_beyond_last_line` guarantees there is somewhere to go.
    **The AI fix is one action with three ways to ask for it**, `fix_with_ai`: the error bar's *AI
    fix*, the error modal behind its *View* (through `Tab::fix_req`), and the right-click *AI fix*
    over a squiggled statement. All three open Ctrl+K pre-filled and go straight to `Busy`, so the
    user approves or rejects a diff and nothing runs. The caller supplies the range, because only the
    caller knows which statement it means — the run error's comes from `intel::error_fix_range` (the
    failing statement, not the buffer), the menu's from `sql::statement_range` at the right-click
    with `intel::problems_in_range` for the messages — and `prompt::ai_fix_prompt` supplies the
    words. It **selects the range** as the other two entry points do, and this is the one that needs
    it most: the range is `error_fix_range`'s choice rather than a gesture of the user's, so without
    the selection nothing distinguishes "rewriting this statement" from "rewriting all forty".
    The menu entry **is only there when that statement has a diagnostic**, shown rather than
    disabled like *Create view*, but the build-time pass decides only *that*: the range and the
    messages are re-derived when the entry runs, like every other action in this menu. Captured,
    they could outlive the text they described — a reload between the right-click and the click
    leaves `sql.get(lo..hi)` answering `None`, and the entry then does nothing with nothing said.
    The modal's is the narrower case: it appears only when
    the modal fell back to the tab's run error, never over an `error_modal_text` override, which is a
    commit error or a server that didn't answer — nothing the editor can rewrite. It has to route
    through a request signal at all because `CmdK` is created inside `query_pane` and never leaves
    it, so the workspace-level modal has no handle to reach it with — and that signal **carries the
    message**, because the modal shows the error it opened on while a run landing behind it moves
    `results` out from under that (its dismiss layer stops clicks, not queries). It also **closes
    before it asks**: signals notify synchronously, so the request opens Ctrl+K and focuses its field
    on the spot, and asking first left that focus to be decided by the teardown of the modal's own
    `focus_root` afterwards.
    **`Explain` is the fix's pair, and it deliberately goes somewhere else**: a fix is a diff in the
    editor, an explanation is prose in the chat panel — the same split the right-click menu already
    makes between its own *Explain* and *Optimize*. It reveals the panel before sending, because a
    message into a hidden panel reads as a button doing nothing, and it highlights the statement it
    asked about, so the answer and the SQL it is about are visibly the same statement. Both the error
    bar and the modal offer it. On the bar the two AI actions sit **together at the far edge**, which
    is what the sparkle marks — *View* opens a window, those two reach a model — and the bar is
    **responsive about them**: `error_bar_fits_explain` measures every gap and label to the right of
    the message and asks whether they fit in the share the message's own `max_width_pct` leaves them
    (`ERROR_BAR_MSG_PCT`, 60/40), dropping *Explain* when they don't. It is the one of the three that
    can go, because the *View* modal offers the same explanation. **A share, not a pixel floor**, and
    the first attempt was the floor: a minimum width for the message with the buttons taking whatever
    was left. It passed a bar where the message ended up a few ellipsized words with the buttons
    packed against it — the arrangement it existed to prevent — because what makes the bar look
    crowded is the buttons' *proportion* of it, and a floor written at 100% is the wrong number at
    every other interface scale anyway. Two smaller things had to be true for the drop to be a
    *choice* rather than the layout's opinion: the buttons are `flex_shrink(0)`, and the message
    carries `min_width(0)` — a flex item defaults to `min-width: auto` and refuses to shrink below
    its content, so a long error did not ellipsize past its cap, it pushed, and the right-hand
    buttons were drawn on top of one another. The bar's two horizontal insets are **one
    `padding_horiz` on the bar** rather than a margin on each end's child: written as two margins
    they were the same number and still did not read as one, because the right-hand button is a row
    (icon, gap, label) whose box ends past its last glyph. Unlike the fix it needs **no request
    signal**:
    the chat panel belongs to the workspace, so the modal can reach it directly, the way the schema
    tree's own *AI Explain* does. It is also offered **wherever the fix is not** — over an
    `error_modal_text` override, where there is no statement to rewrite but the words still deserve
    an answer — and withheld only when the modal was opened on nothing at all, where it would ask
    the model to account for the phrase "No error.".
    `inline_footer_y` is the other end's geometry, and it is deliberately not `points_of_offset` at
    the anchor line's end: that offset maps to a column *before* the phantom rows, so its `bot` is
    the bottom of the line's own row. The **next** document line's top is the honest answer — the
    added rows are exactly what pushed it down — and the last-line case, which has no next line,
    steps past them with `ed.text_layout(anchor).line_count()`, **visual rows and not `add.len()`**:
    with word wrap on a long added line occupies more rows than it has lines, and counting lines put
    the bar one row short with the tail of the suggestion stranded below it. It also takes `area_h`
    and answers `None` when the bar would not fit inside the pane — it is absolutely positioned in
    the pane rather than clipped to the editor, so a block scrolled past the top left Accept/Reject
    floating over the toolbar and the tab strip. That fit test measures the bar through the same
    `VERDICT_BAR_H` the style draws it with, since a height living in two literals is a bar that
    reports itself on screen at one size and paints at another. Inside it the padding is deliberately
    **top-heavy** (1px border, then 7 above the words and 1 below): the row is centred in the content
    band, so an even split centres the words' *line box*, and a line box carries descender space that
    almost nothing in "Accept · Reject · 1 hunk" fills — geometrically centred reads high.
    **The verdict bar takes pointer events and forwards the wheel.** It lies across the document,
    and Floem's child walk `break`s on the first view eligible for a pointer event whether or not it
    handled anything (the pointer-routing gotcha), so a bar with pointer events on kills
    scrolling over itself and *not* handling the wheel does not help. It forwards `PointerWheel` to
    `ed.scroll_delta` instead — which is exactly what Floem's own gutter does with the same problem
    (`view.rs:1112`) — and that is what lets it keep its buttons and set `CursorStyle::Default` over
    them, since it sits on the editor's I-beam and nothing in it is selectable. (It was briefly
    split in two, surface in the click-through overlay and words here; forwarding is the better
    trade, and `pointer_events(false)` is now reserved for the band strips, which have nothing to
    click.) **The forward is on all four views** — the row, Accept, Reject and the change count —
    because the walk breaks at whichever child is under the pointer: on the row alone, a wheel over
    the two words still died, which reads as intermittent rather than as a missing handler. The bar
    is aligned to the code column, and the same two closures answer the keyboard: `CmdK::verdict`
    publishes them to the editor's key handler, because in that state there is no field left to catch
    Enter and Escape and the editor is what holds focus. One accept and one reject in the pane, two
    ways in.
    **Which states the editor answers for is `cmdk_editor_keys`, and Escape's are not Enter's.**
    Enter belongs to `Ready` alone — there is nothing to accept before the suggestion lands — but
    Escape also takes down a request that is still `Busy`. A request is not always started from the
    prompt field: *Optimize* and the three ways to ask for an AI fix open the bar already `Busy`
    from a menu the user clicked, so the editor never gave the keyboard up. Gating the branch on
    `Ready` therefore left Escape doing nothing for the whole of a running request — it closed the
    bar while prompting (the field's own `on_escape`) and while previewing a diff, and not in
    between, which is the state a user most wants out of. `Idle` and `Failed` stay the field's:
    both have a live, focused, unfrozen field that answers Escape itself, and a branch here would
    take the key from the completion popup the user can open in the editor underneath.
    **The forward is also on the outer box**, and that is what the "sometimes it swallows
    the scroll" report was really about: the row is content-height, so the bar's padding, its border
    and the space above and below the words all belong to the box, and a wheel worked across the
    words' own band and died everywhere else. The centring is the same story from the layout side.
    It is **`justify_center()` on the inner `container(content)` wrapper** — the one that actually
    fills the box's definite height, and whose default `justify_start` was pinning the row to the
    top. Putting it on the absolutely-positioned box did nothing, because that box's child chain asks
    for `height_full` and so already fills it; the row's own `height_full` + `items_center` had
    nothing to resolve against either. Centre on the view that fills the height, not on the one that
    defines it.
    **Both bars run edge to edge**, and that is a **deliberate divergence from the design**, which
    insets the question bar from the right: both belong to the block of lines they sit under, and one
    of the two stopping short read as an inconsistency rather than as a detail. It costs something,
    which is why the scrollbars moved — see below. They are inset 1px on each side to clear the
    editor's own border, and the verdict bar carries 2px of vertical padding. Those 1px insets are
    `EXEMPT` entries in `consts::float_inset_gate` rather than `float_inset()` calls, for the reason
    the entries state: these are not floating boxes keeping air from a panel edge but rows of the
    block above them, and the number is the border's width, which does not scale.
    **The buffer is frozen while a suggestion is on screen** (`ed.read_only`, released on accept or
    reject — and read the gotcha on that flag before touching it, because the auto-pair handler owns
    it too and it does not gate `Document::edit`): the phantom rows are anchored to line *numbers*
    and the plan was computed against the text as it was, so an edit underneath would leave the rows
    describing lines that had moved and Accept
    splicing at stale offsets. The old overlay got that for free by covering the editor; this
    one deliberately does not, so the freeze has to be asked for. For the same reason **a click in
    the editor dismisses Ctrl+K only while it is still `Idle`**. Treating any click as "never mind"
    was safe only while the working and diff states covered the editor and a click in them could not
    land there at all; neither covers it now, and the diff in particular sits *in* the lines, so
    clicking the very thing being decided on threw away a generation the user had waited for.
    `inline_band_runs` finishes those bands, visiting the lines the diff touches **that are on
    screen** (`visible_hunk_lines`) and asking
    `inline_diff::row_split` — the same function the code column's own bands go through — which of
    that line's **visual** rows are the block's and which are its own, so the two halves of a band
    cover the same rows even where word wrap has given a line more rows than it has lines. Its
    `top_of` answers `None` for a line **past the end of the document** as well as for one off
    screen: the plan is computed against the buffer as it was, so a hunk can outlive the lines it
    names, and clamping such a line to the last one (as it used to) painted its band against
    whatever text happens to be there now. **`row_split` alone does not make the halves agree**:
    it answers *which rows*, not *which lines*, and the deletion band needs the second answer too —
    `hunk.del.contains(&line)`, the same gate as `sql_highlight`'s `replaced`. Where that is
    load-bearing is the **pure insertion**, whose visited line is the anchor: an untouched context
    line the block merely hangs off. Ungated, the strips painted a red band and a `−` in the gutter
    beside a line nothing had happened to while the code column stayed clean — the two halves
    agreeing about rows and disagreeing about lines, in exactly the case the `del.is_empty()` branch
    exists for. Reading `row_split`'s guarantee as covering more than it does is the same shape of
    mistake as the row count it replaced: **a helper that answers most of a question invites the
    caller to stop asking the rest.**
    `sql_highlight`'s `LineExtraStyle` covers the code column correctly, but the
    editor's content lives inside a clipping `scroll` whose gutter is a *sibling painted before it*, so
    nothing drawn from inside can reach the gutter or the wrapper's right padding. Those two strips
    carry no text, so an overlay finishes the band across them without covering anything — and the
    left one carries the row's `−`/`+` in place of the line number it hides, which is what the
    design puts there. The gutter strip stops **`HL_PAD` short of the code column**: `HL_GUTTER` is
    a measured estimate that runs a shade generous, which is fine for the statement-highlight
    *border* it was tuned for (that one pads outward by exactly `HL_PAD` to clear the glyphs), but a
    filled band inherits no such margin and was painting over the first character. The overlay is
    `.clip()`ed to the editor for `inline_footer_y`'s reason — line geometry against a surface that
    scrolls under it — and is nested with `editor_box` inside the `editor_area` stack (which takes
    at most 16 children, and the pair occupies the same rect either way), sitting **directly over
    the editor, under every other overlay**: the overlays above it carry text and must not be
    covered.
    **The custom scrollbars paint *above* the Ctrl+K bars**, listed after `cmdk_view` as **two
    separate children** — the stack is at exactly 16, which fits. The bars run edge to edge and the
    scrollbars are pinned to the editor's *border* rather than to its content edge, so at their old
    layer an open Ctrl+K covered them; being drawn last costs the bars nothing (the scrollbars are
    thin and sit at the extremes) and puts a drag where the user aimed it. **Never wrap the pair in
    a `stack` to save a child slot**, however tight the 16 gets: a wrapper around them is
    `absolute().inset(0)`, a pane-sized view above the editor that takes pointer events, and it ate
    every click, drag and wheel in the editor for as long as it existed (see the pointer-routing
    gotcha, which states the general form).
    **`visible_hunk_lines` is a filter, never a clamp.** A line outside the viewport is dropped, not
    pulled to the nearest one — the rule `top_of` already states for a line past the end of the
    document, and for its reason: a band placed against whatever text happens to be at the clamped
    line is worse than no band. It returns the hunk's deleted range narrowed to the visible lines
    plus the anchor of a pure insertion when *that* is on screen, since an insertion has no deleted
    line to hang its added rows off. `None` for the viewport is an editor with nothing laid out yet,
    which places no offset at all. The narrowing is why it is a function: `inline_band_runs` re-runs
    on **every scroll tick** and used to call `points_of_offset` for every line of every hunk before
    finding out the line was off screen — floem answers that with a linear scan of `screen_lines`.
    A whole-buffer suggestion, which `fix_with_ai` produces routinely whenever
    `intel::error_fix_range` cannot locate the error's token, therefore cost `O(deleted × visible)`
    per frame with none of it drawing anything; bounded by the viewport it is `O(visible)`.
    **A known cost, accepted rather than fixed**: `inline_band_runs` returns pixel positions and is
    the strips' `dyn_container` key, so every scroll frame rebuilds those views while a diff is on
    screen. Making it reactive means keying on `(line, row)` and moving the geometry into N per-strip
    style closures that each call `ed.text_layout(line)` — a real refactor with its own failure
    modes, for a transient overlay of usually under ten rows. The per-call cost dropped a long way
    when the strips moved to `block_at`, which is what made leaving it the reasonable trade.
  - `inline_diff.rs` — the Ctrl+K suggestion rendered **in the editor's own line flow**: the lines
    it replaces stay where they are, faded, and the lines it proposes appear directly below them,
    pushing the rest of the document down. They are Floem *phantom text* (the facility inlay hints
    use), so **the rope is never touched** — `doc.text()` keeps returning the user's own SQL for the
    whole preview, and tab autosave, Ctrl+Enter, live validation, completion and the outline all
    keep seeing the buffer the user actually has. Splicing the diff into the rope would have been
    far less code and would have put text the user never wrote in front of every one of them.
    Two halves over one `RwSignal<Option<InlineView>>` (`InlinePreview`): `InlineDiffDoc`, a
    `Document` wrapping the editor's real one that delegates everything except `phantom_text` — so
    the rope, the undo history, IME preedit and every edit command are untouched — installed with
    `TextEditor::use_doc` just before the `SqlStyling` that paints over it; and `segments`, the row
    builder **both** halves go through, so the styling that paints the rows and the document that
    emits them cannot disagree about what they hold or where they start (how many rows they *occupy*
    is `row_split`'s question, below). **`segments` is the expensive one and only `phantom_text` needs
    it** — it re-tokenises and re-allocates the whole suggestion for its colours — so the styling and
    the gutter strips ask `block_at` instead, which answers the only three things they wanted: is
    there a block here, which side of the line, how long. They run *per visible line, per relayout*,
    and were re-highlighting the suggestion several times a frame to learn that. The two must agree
    byte for byte, because `row_split` uses `Block::len` as a column into the combined line — one
    `\n` per added row plus each row's text, and an **empty row still costs the one byte** of the
    space `segments` substitutes for it, which is the one place the cheap length could silently
    drift. `a_blocks_length_matches_the_bytes_segments_emits` and
    `an_empty_added_line_costs_the_space_it_renders_as` pin that seam.
    `set_preview` is the only correct way to set the signal: it also bumps `cache_rev`, because the
    rows are baked into the line's cached `TextLayout` (see the Floem gotcha on phantom text for that
    and the rest of the list's rules). It **returns early when the view is unchanged**, which is
    worth the comparison rather than a micro-optimisation: the bump discards every cached line layout
    in the editor, and the states that publish `None` are the common ones — every Escape, Accept,
    Reject and Ctrl+K passes through one, almost always with `None` already in place.
    `InlineView` is the *two* things a Ctrl+K request puts on that surface, not one.
    `Working(Range<usize>)` is the request in flight — the document lines it covers (from
    `diff::line_span`, since the request is a byte range) fade to say so, and nothing else happens:
    no rows are added and **no band is painted**, because nothing has been proposed yet.
    `Plan(InlinePlan)` is the settled suggestion, whose replaced lines fade *and* get the band while
    its own lines arrive as phantom rows. The two fade depths differ deliberately — `fade()` returns
    0.45 for `Working` and 0.65 for `Plan`: waiting is the stronger dim because the faded text is
    all there is to look at, while a replaced line still has to stay readable against the one
    proposed under it. Both that depth and the `fades(line)` predicate live on the enum, so
    `sql_highlight` applies **one** rule rather than a rule per state; `segments` takes the view and
    returns `None` for `Working` through `InlineView::plan()`.
    The colour is `sql_highlight`'s half of the same signal. `apply_attr_styles` fades a line the
    view says it fades — alpha on the token colours **plus a whole-line span**, or an identifier
    `lex_line` doesn't colour would stay at full strength in the middle of a faded row — and shifts
    the line's spans by `Block::prefix_len()` for the one `before` case; Floem calls that hook
    with the line's *pre-phantom* columns and adds the phantom spans afterwards, so an end-of-line
    block (every block but that one) needs no adjustment at all. `apply_layout_styles` early-returns
    on `Working` for the reason above and otherwise pushes the row bands, `diff_del_bg` behind the
    replaced rows and `diff_add_bg` behind the added ones, with **`width: None`** — that is what
    makes Floem paint across the whole viewport rather than just behind the glyphs, i.e. what makes
    a diff row read as a row. It cannot reach further than that, though: the gutter and the
    wrapper's right padding are outside the editor's clipping scroll, so those two ends of every
    band are finished from *outside* by `editor_pane::inline_band_runs`. The split is structural,
    not duplication.
    **Which rows to band is asked of the layout, never counted from the plan** — `row_split`, and
    both painters go through it. The plan knows how many *lines* the suggestion adds; what has to be
    banded and marked is *rows*, and the two part company the moment a line wraps. `Segments`
    therefore carries no row count at all any more: a struct holding an answer to that question was
    the standing invitation to get it wrong. `row_split` asks `hit_position` at the phantom's start
    column, from the same text layout the glyphs came out of, so it cannot disagree with what is on
    screen. The trap it encodes: for an end-of-line block the phantom's first byte is the `\n` that
    ends the line's own content, and the layout puts that index at the start of the **next** row, so
    `row_of(own_len)` already names the block's first row. Adding one to it — assuming the column
    names the row the preceding text *ends* on — pushed every band a row late: without wrap the added
    rows fell outside the range and got no band at all, so the whole diff read as deletions, and with
    wrap it banded the suggestion's first row as a deletion.
    **The arithmetic is `split_rows`, split out from the layout question so it can be tested.** Both
    defects this code has shipped — a row count taken from the plan, then that off-by-one — lived in
    those few lines, and neither was reachable by a test while exercising them needed a real
    `TextLayoutLine`. Nine tests now cover it, `a_start_row_at_the_end_bands_nothing_as_added` being
    the shipped off-by-one itself. `row_split` keeps only the part that genuinely needs the layout:
    which visual row the block starts on.
  - `erd_view.rs` — the **ER-diagram** canvas over `core::erd`. Edges are drawn by a custom paint
    view (`EdgeCanvas`), *not* a Floem `svg` — `svg` doesn't repaint reliably on reactive change
    here and blanked the edges on drag/hover. Zoom is **semantic, not a paint transform**: cards and
    edges keep logical positions and multiply by `z` only at render, so text stays crisp at any
    zoom. The surface is an infinite free pan (not a scroll view) — drag/middle-drag pans, Ctrl+wheel
    zooms about the cursor, plain/Shift+wheel pans — and hit-testing maps cursor → logical space via
    `(p − pan) / z`.
    **The toolbar is responsive**, and the decision is `core::erd::fit_toolbar`'s while the
    measurement is this module's: `chip_w` / `icon_button_w` / `zoom_unit_w` predict each group's
    width *before* layout, from the same `TOOLBAR_*` constants the widgets are drawn with — the
    `MENU_ROW_PAD` discipline, for the reason that one records. The toolbar reads its **own**
    measured width via `on_resize` rather than re-deriving the panel's `ww * 0.8`; it is
    `width_full`, so its width does not depend on the children being hidden and there is no
    measure-hide-measure loop. The scope breadcrumb is the one variable-length item and so the one
    that ellipsizes, and it does so only after every optional group has already gone.
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
    app's shared error modal, which is painted *under* this one. It arrives as the `ExportOutcome`
    the results export shares, of which a diagram uses two: one document has no row count and
    nothing to cancel, so `export_erd` reports `Done(0)` and never `Cancelled`. The handler still
    **spells all three arms out**, the way `monitor_view::save_log` spells its own and for the
    reason that call site gives — the arm is unreachable today, but the rasterise it wraps is one
    change away from being cancellable, and the catch-all `_ =>` this had would then report a
    cancelled export as "Saved <name>". A confirmation fades after
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
    `source: None` and a default dialect, both of which only the SQL renderer would read. The log is
    always `ExportScope::Fetched` — it is a record of what the monitor saw, not a query anything
    could re-run — so its `ExportOutcome::Cancelled` arm cannot arrive, and it is spelled out as an
    explicit no-op rather than folded in with `Done`: marking the log exported off a half-written
    file would stop Clear asking about it (`monitor::discard_needs_asking`).
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
  - `theme.rs`/`themes.rs`/`icons.rs`/`fonts.rs`/`sql_highlight.rs`. `theme.rs` is the call-site
    surface (named colour fns, the type scale, `header_h`/`footer_h`, `scaled`/`scaled_font`);
    `themes.rs` holds the data and the three runtime axes, including `UiScale` and the pure
    `scale_at`/`scale_font_at` rounding; `icons.rs` sizes every glyph, scaling the **base** size its
    callers pass.
    `preview_bg`/`preview_fg` are the surface and base text of a **syntax-coloured preview**, and
    they are named here rather than spelled at each site so the cross-axis gate in `contrast.rs`
    measures the surface the previews actually paint — the account browser's `GRANT` block is the
    latest caller, and its move off `code_bg` is exactly that distinction and nothing else:
    `preview_bg()` *is* `code_bg()`, the same pixel, but spelled through the accessor the gate reads
    rather than through one it does not. Both come from the *editor* axis, which is the
    whole point: the token colours are reproductions of palettes tuned against their own background,
    the editor theme is chosen independently of the light/dark UI theme, and pairing them with a UI
    surface is a combination nobody chose — see `contrast.rs` for the ratios that cost.
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
    The other thing the tables cannot express is a **cross-axis** pairing, and
    `a_coloured_preview_is_legible_in_every_ui_and_editor_theme_pair` is where that one is measured.
    Every row above holds one theme against itself, so neither table could ever see the combination
    two panels really paint: the *editor* theme's token palette on a surface the **UI** theme chose.
    The History and Snippet previews took their token colours from the editor theme and their
    background from `bg_editor` — which despite the name is the UI theme's text-**field** surface —
    and on the Light UI theme with the shipped-default Tokyo Night that put `string` at 1.70:1 and
    `number` at 1.90:1, under even the *Recessive* floor of 2.0, while the uncoloured text beside
    them sat at 5.70:1. The text the colour was added to make stand out was the only text made
    unreadable. The test walks the whole `(UiThemeKind, EditorThemeKind)` product asking
    `theme::preview_bg`/`preview_fg` — the accessors both previews call, so it measures the decision
    rather than a copy of it — and pointed back at `bg_editor` it fails on 6 of the 12 pairs. Tokens
    are held to *Recessive* for `EDITOR_PAIRINGS`' reason (each is a reproduction of a published
    palette, so the assertion is that it is a colour and not a stain) and the uncoloured base to
    *Body*, being the panel's real body text. The active theme is process state, so the loop restores
    it through a `Drop` guard: a panic inside it would otherwise leave the thread on Light + Latte
    and surface as some unrelated later test failing.
  - `lib.rs` (~7.6k lines; `grid.rs` at ~10.2k is the crate's largest) — the `Ui` struct + bundles,
    shared model/state
    types, `workspace`/`body`/`center`/`header`/`footer`, `edit_field`/`FieldCfg`,
    terminal panel.
    Two things about `edit_field`'s **multiline** boxes, both found in a body field and both fixed
    in the shared helper rather than at one call site. **Enter is only swallowed when there is
    something to submit to**: a multiline field with no `on_submit` lets it through and breaks the
    line, which is what Enter means in a box of SQL — consuming it unconditionally left every body
    field (the snippet editor's, the view editor's) with a dead Enter and a Shift+Enter nobody would
    guess at. The corollary is a trap for callers: **the no-submit arm is only safe for a field that
    is also read-only**, or Enter silently types a newline where the user meant an action. Separately
    from that, `multiline` was answering two questions at once — *wrap and auto-grow*, and *Enter may
    break the line* — which are the same answer for a snippet or view body and opposite answers for a
    box holding one question, which wants to grow with a long question and has nothing a second line
    could mean. `FieldCfg::enter_never_breaks` separates them: the Enter branch gates on
    `breaks = multiline && !enter_never_breaks` in *both* arms, and that is the **only** place the
    flag is folded in — `plain` stays `!shift && !control`, because once a field cannot break its
    line no modifier combination changes what Enter means, so the guards stop consulting `plain` at
    all. Wrapping and auto-grow are untouched. Ctrl+K's
    prompt sets it, and its `on_submit` is **never `None`** (while a request is in flight the closure
    is a no-op), so the key is swallowed on its own terms rather than by depending on read-only to
    refuse the edit afterwards.
    **All of which is right, and none of it was the reported bug.** The "stray newline" in the Ctrl+K
    question was never a newline: the caret could not reach a second line, and it happened when
    submitting with the send triangle too, so the Enter key was not involved at all. The bar was
    *taller* — its `dyn_container` was keyed on the request state, so every transition **rebuilt the
    field**, and a freshly built `edit_field` has almost no width for one frame. A one-line question
    wraps in that frame, the row count is measured from that layout and latched, and the bar reopens
    a row taller with its text stranded at the top, which is indistinguishable from a typed newline
    in a screenshot. Three rounds of fixes went at the Enter key on that resemblance alone. The
    lesson is the misdiagnosis: **a taller box and an extra line look identical, and only the caret
    tells them apart** — ask where the caret can go before touching the key that would have put it
    there. The fix is in `editor_pane` (the container is keyed on `Ready`-or-not now); what is kept
    here was worth keeping on its own terms and has no regression test either way.
    **That was only half of it**, and the other half outlived the rebuild by a long way: the bar went
    on opening two rows tall about once in seven times, with no rebuild left to blame. The
    re-measure effect tracked `viewport` alone — but floem answers a width change in an effect of its
    own *on that same signal*, `lines.set_wrap(Width(viewport.width()))`, and that only **clears** the
    line layouts; they are rebuilt lazily afterwards. So the count read on the `viewport` edge was
    computed from the layout of the **previous** width, and `Lines::last_vline` caches its answer —
    which is what made a bad frame permanent rather than momentary, since nothing but a keystroke
    would ask again. The effect now also tracks `screen_lines`, which floem `update`s once those
    layouts exist again (`update_screen_lines` walks the visual lines, and walking them is what
    builds them), so the measurement happens at the first moment the count is true. The general
    lesson is the one the misdiagnosis above teaches twice: **a value read on the edge that
    *announces* a change can be older than the change**, and a cached reader turns that into a
    permanent wrong answer instead of a frame of one.
    And **the field repaints when its row count changes**: the box height is derived from
    `rows`, so the edit that adds a line has already painted against the *old* height, leaving the
    caret on the new last line outside the viewport that was drawn — it looks like the caret has
    vanished until the next keystroke repaints against the grown box. The repaint is deferred a tick
    so it runs against settled layout.
    (`FieldCfg::min_rows` is the floor such a box starts at — 1 by default, 3 for a snippet body,
    clamped against `max_rows` at use because `clamp` panics when the floor exceeds the cap.) The shared types living in the crate root is what stalls further splitting: the
    root depends on the leaves (`mod`) and the leaves depend on the root (types), so a view builder
    can't move out until the types do.
    **`modals.rs` is the first cut that did move**, and it is the shape the rest should follow: not
    the biggest view builder, but the one piece of `workspace` with an *invariant* attached. It took
    the modal layer, its four predicates and `modal_backdrop_gate` with it, and `workspace` kept a
    single tuple entry plus the one `modal_backdrop_up` call the title-bar band also reads. Nothing
    about the types had to move, because the layer needs only `Ui` — which is the test for whether a
    piece is ready to leave. **`dividers.rs` is the second**, and it is the other kind: no invariant,
    no test, purely a self-contained pair of views (~240 lines) that `body` and `center` call. Both
    cuts left the shared types where they are, so the stall above is unchanged — the next cut still
    has to answer it.
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
  arithmetic. **The step alone is not the offer, because it can name rows that do not exist**:
  `stats::read_more_offer` takes the same `row_total` the stats line prints and, when the step
  already covers it, says *"read all rows"* instead. That case shipped wrong — a 200k read of a
  ~292k table stepped to a million and the toolbar read "read 1m rows", on a line that had just
  described the table as ~292.02k. It then shipped wrong the other way: *"read all ~292.02k rows"*
  named the total a second time, three words from where `rows_read_clause` had already printed it,
  and the pair was the widest thing on the strip. **The offer names a figure only when the figure
  is its own** — the step is a number that appears nowhere else on the line, and dropping it would
  leave "read more" implying a cursor the row cap has none of; "all" needs no number, because
  there is only one thing it could mean. The total is consulted, never trusted, **and an estimate
  is given room to have been wrong**. Rounding it up to two significant figures tidies its tail but
  is not slack, and re-capping is not the self-correcting answer it was written as: `employees`
  samples to 292,025 and rounds to 300,000, which is 25 rows short of the table, so *"read all
  rows"* came back capped — and the offer behind it, with 292,025 now stale against 300,000 read,
  fell to the step and asked for **1.5m rows to fetch those 25**. So an estimate is padded by
  `stats::ESTIMATE_SLACK` (a half — InnoDB's own documented 40–50% sampling error) before it is
  rounded. The padding is free, because the cap cuts off a stream that ends when the table does: one
  set above the real count is never reached. What bounds it is the step it replaced — *"read all
  rows"* never asks for more than the numbered offer it was chosen over. A total at or below the
  rows already read is stale and says nothing, the same rule `rows_read_of` follows. The offer is a
  `label`, not a `text`, for the same reason `stats` beside it is one — the total arrives from a
  catalogue query after the strip is built — and the click re-asks rather than capturing, so the
  cap always matches the words the user just read.
  **The offer takes itself off while it is being answered.** The re-run is a view run, which
  deliberately leaves the current table on screen, so nothing else on the panel says one is
  happening: the grid goes on looking idle and the words go on inviting a second click — a second
  full read of the very table the label has just promised to read *all* of. `Tab::view_busy` is
  that flag, and the app's run path owns it because only it knows when the re-run lands: set on the
  last line before the spawn (so every early return above it leaves the flag alone), cleared on
  every arm of the landing including `Cancelled` — a link disabled for ever is worse than one that
  can be clicked twice — and cleared **after** the supersede check rather than before it, since a
  run that has been replaced is not the one the flag is about any more. Whatever did the
  superseding clears it instead, beside the `token.cancel()` in `run_query_core` and `run_all`,
  because the superseded run returns at that check and never reaches its own clear. The label, its
  colour and the click all read the one flag, so they cannot disagree about whether it is still a
  link — and the click is guarded on it rather than on hover, since a pointer already over the
  words when the first re-run started never leaves and re-enters them.
  The re-run goes through `GridState::current_statement`, **not** `apply_grid_query`:
  the latter reports a base it cannot rewrite as a *filter* failure ("not a simple single-table
  SELECT"), and a join is perfectly re-runnable at a bigger cap — telling a user with no filter
  that their filter is at fault is worse than the cap they were trying to get past
  (`an_ineligible_base_is_still_ineligible_with_nothing_to_splice` pins the premise). **And the same
  call is the write guard**: `current_statement` wraps `filter::rerun_statement`, so a statement that
  may not be executed a second time draws no link at all — the offer's predicate and its click ask
  the one function, where they used to ask only whether `base_sql` was `Some` (*Architecture
  invariants*, the write guard). Clearing the
  override on a fresh manual run is the other half: a raised cap belongs to the result it was
  raised for, and carrying it forward would be the global setting the user didn't change.
  **The export is the other way past the cap, and it is a different shape.** `export_file` branches
  on `ExportScope`: `Fetched` is the one `spawn_blocking` it has always been and writes through
  `ExportFormat::render_to`, which is exactly a `OneChunk` plus `stream_to` — spelling that pair out
  at the call site instead left the wrapper with no production caller and two copies of one
  construction to drift apart. `AllRows` is
  **two tasks and a bounded channel of 2** — the reader is async (two of the three drivers are) and
  the writer is synchronous file IO, which must not run on a runtime worker. The writer's
  `PullChunks` closure turns a channel `Err` into an `io::Error` and a channel close into
  end-of-stream, and **the reader's verdict wins where the two disagree**: a cancelled read closes
  the channel, which the writer sees as an ordinary end of stream, so the writer alone would report
  a truncated file as a finished export. `EXPORT_CHUNK_ROWS` is 10,000, with the trade stated at
  the constant and nothing downstream depending on the figure — but **it is no longer the only thing
  that ends a block**: `RowDest::chunk_full` also cuts at `db::CHUNK_BYTE_BUDGET`, because a row
  count is the wrong unit for a promise about memory (see `schemaic-db`'s streaming account). `export_token` is an
  `Rc<RefCell<Option<CancellationToken>>>` in the same shape as `import_token` — cleared when a run
  reports, so a later Cancel can't cancel an export that already finished.
  **One slot means one streamed export, and the second is refused rather than queued or keyed.**
  Two sharing the slot is what the shape would otherwise permit: Cancel reached only the later one,
  and whichever finished first cleared the slot and left the other with no way to stop it. The
  `AllRows` branch therefore answers `ExportOutcome::Failed("An export is already running. Cancel it
  or wait for it to finish.")` while the slot is full, rather than growing into a keyed map — a
  single slot is correct once nothing can overwrite it. Only that branch asks: a `Fetched` save
  takes no token and is never refused. `import_token` gets away with the same shape and no refusal
  only because its modal admits one run at a time, while the Download menu sits on every result tab.
  **The offer wears the accent, and its separator does not.** It shipped `text_dim` like the
  description beside it, reaching the accent only under the pointer — which put the one escape from
  the row cap behind a hover nobody had a reason to perform. It is now accent at rest (registered
  as `accent on bg_results` in `contrast.rs`, Body threshold) and **stays blue on hover**, stepping
  to `accent_hover`: trading the accent for `text` on hover read as the link switching off at the
  moment it was aimed at. `accent_hover` is a themed role and deliberately not "a lighter accent" —
  it moves *away from the surface*, which is lighter on dark (`#7C9CF0` → `#A3BCF8`, 6.5:1 → 9.18:1
  on `bg_results`) and **darker on light** (`#3D66D6` → `#2B4FB0`, 5.03:1 → 7.19:1), because
  lightening the light palette's accent on a near-white surface walks a Body-weight label towards
  failing AA. A hover that reduces contrast is not a hover. Both states are registered pairs, so
  the suite measures them in every theme. That makes it two views rather than one string — the `·` stays `text_dim`,
  because a blue separator reads as part of the link — and the colour is driven off an explicit
  `offer_hov` signal rather than `.hover()`, since a parent's hover colour does not cascade to a
  child and the click target is the pair.
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
  `shown_panel` again rather than a second fallback rule (a stale `active_result` shows the first
  panel in the pane, and the AI has to describe the same statement) — so columns, types, counts, the cap and
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
  **What comes back is gated, not trusted**: `inline_outcome` runs `extract_sql` (fences off) and
  then `intel::sql_reply` (the parse gate above), and a reply that will not parse becomes
  `Failed("The model did not return SQL")` rather than an edit. The composition is what the caller
  relies on, so it is pinned *here* as well as in `intel` —
  `inline_outcome_drops_a_tool_diagnostic_riding_on_the_sql` puts the chatter inside the fences,
  where neither function alone would have to deal with it, and
  `inline_outcome_refuses_a_reply_that_is_only_prose` holds the refusal.
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
  - `conn_sources.rs` — the I/O half of connection import: which paths on *this* machine are worth
    opening, and reading them. Deliberately only that half — nothing here interprets the bytes,
    which is what keeps `core::conn_import` unit-tested and leaves the part that cannot be (a walk
    of the user's home directory) small enough to read. `discover` returns the sources in the order
    `conn_import::scan` relies on: the two GUI clients first, because they are the only sources
    carrying a name a human chose, then the plain-text files, whose value is the passwords they can
    lend to the rows above them.
    **Every root is probed on every platform** — no `cfg` branches, even though the locations are
    per-OS: an absent `~/Library` on Linux costs one failed `read_dir`, and a `cfg` costs a path
    only a third of the developers can exercise. DBeaver is two levels of wildcard
    (`DBeaverData/workspace<N>/<project>/.dbeaver/data-sources.json`) and both are real — taking
    only `General` misses every connection of anyone who made a second project. JetBrains is
    `JetBrains/<Product><Version>/options/dataSources.xml` and is **not** filtered by product,
    since the file's shape is identical under IntelliJ or PhpStorm. Per-project
    `.idea/dataSources.xml` files are not searched at all: they are wherever the user keeps code,
    and the modal's "Choose a file…" opens one directly. `PGPASSFILE`/`PGSERVICEFILE` are honoured,
    because libpq reads them first. Reads are capped at 4 MiB and a file that fails is silently
    skipped — this runs over paths the user never named — *except* one they picked by hand, which
    goes through the same `read_source` and is reported by the modal.
    `password_sources` is the narrow half of that walk — today just `~/.pgpass` — for the one path
    that reads a single file: a hand-picked DataGrip export would otherwise arrive with twelve
    blank passwords that libpq's file, on the same machine, holds every one of, and whether a row
    can be completed must not depend on how its file was found. Four `is_file` checks and one small
    read, cheap enough to run inside a file-picker callback where the two directory walks would
    not be.
  - `script.rs` — the I/O half of `core::script`, and `dump.rs`'s mirror image: that module reads a
    database and writes a file, this reads a file and writes a database. Two halves at once — a
    **blocking reader** walks the file in `BLOCK`-sized reads, feeds `script::Splitter` and pushes
    completed statements into a bounded channel, while `Db::run_script` pulls from the other end.
    Each reports how it ended and `script::run_outcome` decides which ending the user hears, which is
    **not** the same precedence `dump_verdict` uses (see `core::script`).
    `tx` is moved into the reader and dropped when it returns, so *every* exit — end of file, cancel,
    disk error — closes the channel and lets the executor finish; and `ReadEnd::Stopped` is returned
    with no message on purpose when the send fails, because that means the executor went away and it
    is the one holding the reason.
    **The bounded channel is the progress design.** The reader cannot get more than `SCRIPT_QUEUE`
    statements ahead of the server, so `Splitter::consumed` tracks what has actually been applied
    closely enough to report from — which is why there is no progress channel out of the executor,
    and why a 2 GB file read at disk speed cannot pile up ahead of a server applying it one statement
    at a time. Progress is reported in **bytes**: a file's statement total cannot be known without
    reading it, and its byte length is known at `open`.
  - `dump.rs` — the I/O half of `core::dump`: introspect, plan, write. Its shape is `export_file`'s
    `AllRows` branch with one difference that drives the whole module — an export is one statement
    into one file and a dump is **many**, so the writer has to outlive each table. A single blocking
    task owns the file and reads `Msg`s: a `Text` is written as it arrives, and a `Table` carries the
    *receiving end* of that table's row channel, which the writer drains through `ExportFormat::Sql`,
    so a dump's `INSERT`s and the grid's SQL export are the same statements by construction. Rows are
    read at `EXPORT_CHUNK_ROWS` with the same bound of two blocks in flight, and the `.part`-sibling
    + atomic-rename guarantee is the export's unchanged — `part_of` builds that sibling through
    `export::part_path`, the one function that decides the suffix, because the modal's cancel
    sentence names the same fragment through it and two spellings of `.part` could drift apart in
    exactly the situation where the fragment is the thing the user still wants. Unchanged too is
    **cancel is the reader's to declare
    and every other failure the writer's to describe** — a cancelled read closes the channels, which
    the writer sees as an ordinary end of stream and would otherwise call a truncated file finished.
    That five-arm resolution is now `core::dump::dump_verdict`, with tests: written out here it sat
    inside an `async fn` needing a `Db`, a runtime handle and two channels to reach, so swapping two
    arms turned *The disk is full* into *connection reset* with the suite still green.
    **One check the export path has no need of**: after the join, `token.is_cancelled()` is asked
    directly, because a cancel that arrives while no table is streaming — anywhere in a
    structure-only dump — never reaches the reader's error, and the writer's refusal to
    publish surfaces as an *error*, so a stopped dump would be reported as a failed one. **The schema
    is freshly introspected, never the tree's cache**: a dump is a backup, and a `CREATE TABLE` for a
    shape the server no longer has is a backup that restores the wrong table. That read takes the
    run's own token (`Db::fetch_schema`'s entry has why), so the `Err(DbError::Cancelled)` arm ahead
    of the plan is reached rather than being the dead code it was while Stop could not touch the
    longest phase of a large dump. The two counts in the
    report are deliberately different figures and must not be made to agree — progress counts the
    `DumpStep::Rows` steps (`DumpPlan::streamed_tables`), since a view has no rows of its own and a
    structure-only dump streams nothing at all, so counting tables would promise a "12 of 12" that
    never arrives, while `DumpOutcome::Done`'s `tables` is `DumpPlan::tables`, what the file actually
    covers, and its `missing` is what the fresh introspection could not find at all.
    **Beside it rides an `ExportTally`, not a row count.** `write` folds each table's tally into one
    through `ExportTally::absorb` — rows summed, and a withheld or blanked column named once however
    many tables it appears in, the rule `ExportTally::note` already follows within a single export —
    because the driver used to
    discard `withheld`/`blanked` and an export that wrote every BLOB as `NULL` came back as a green
    *Wrote 5 tables and 115k rows.* What the file could **not** carry is the difference between a
    backup and something that looks like one, so it has to survive the trip back. Progress
    reaches the modal as a **crossbeam channel + `create_signal_from_channel`** rather than a
    callback, for the reason the AI stream and `update.rs`'s download do: `create_ext_action` is
    one-shot and has to be built on the UI thread, so a worker reporting *repeatedly* needs a signal
    fed from a channel. **The dump owns its own cancel slot** (`dump_token`) rather than sharing
    `export_token`: the *grid* export's slot exists to stop two result-set exports competing for one
    disk, while a dump can only be launched from a modal that already refuses a second one, and
    folding the two together would let a running result-set export's Cancel stop a dump the user
    cannot even see. (Both features are called Export in the interface, which is why that sentence
    names which one it means; `core::dump`'s entry has the naming split in full.) The sentences this
    module produces are worded for the interface — `Export failed: {e}`, and
    `Nothing to export — no table matched the selection.` for a plan with no steps.
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
    set. **But `RUST_LOG` cannot ask for a credential.** The filter is built by
    `log_directives`/`filter_for`, which take what the environment asked for (or `DEFAULT_FILTER`)
    and then append `russh=warn`, `russh_cryptovec=warn` and `russh_util=warn` — `CREDENTIAL_TARGETS`,
    a floor the environment cannot lift. Three facts composed into the bug it closes: `RUST_LOG`
    *replaces* the default filter rather than adding to it, so a bare `RUST_LOG=trace` takes the
    target allowlist away with it; `tracing-subscriber` bridges every dependency's `log` records into
    `tracing`; and `russh`'s `session_write_encrypted` traces the packet body **before** the writer
    encrypts it. So `RUST_LOG=trace` put the pre-encryption `SSH_MSG_USERAUTH_REQUEST` — the tunnel
    account's password, as a decimal byte array — into `schemaic.log`, the file Settings → General
    offers an **Open folder** button for, which is to say into the folder whose own doc calls it
    "everything anyone would be asked for". **The order is what makes it a floor**: a bare `trace` is
    a global directive and a target directive is more specific (`EnvFilter` matches
    most-specific-first), while an explicit `RUST_LOG=russh=trace` is an *equal* key that
    `DirectiveSet::add` replaces with the later one — this one. Underscores, not hyphens, since a
    target is a Rust module path (`russh-cryptovec` reports as `russh_cryptovec`), and `russh` covers
    its submodules by prefix. It is russh's surface alone on purpose: `mysql_async`,
    `tokio-postgres` and `rusqlite` depend on neither `log` nor `tracing`, so there is nothing of
    theirs to cap. A malformed `RUST_LOG` falls back to the whole default filter rather than being
    applied directive-by-directive with the bad ones dropped — a filter half the user asked for is
    harder to notice than none of it. Rotation is a size check **once per launch**, not per write: `MAX_LOG_BYTES` is 4 MB and
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
    **That test puts the hook back**, and the reason is this module's own subject: tests share one
    process, so a replacement left standing is one every later panic goes through — including a
    genuine failure in another test, which then prints nothing to stderr and reports as an
    undiagnosable blank. It restores **before its assertions, not in a `Drop` guard**, because
    `std::panic::set_hook` panics if called from a panicking thread and a guard restoring during an
    assertion's unwind would abort the run.
    `the_hook_test_puts_the_process_back_as_it_found_it` is the pin: it installs a hook it can
    recognise, calls the test above directly (ordering is not left to libtest), fires a swallowed
    panic and checks the recognisable one is what the process still holds.
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
  user SQL goes through `TabsActions::run`/`run_all` — or, where there is no moment at which to
  ask the user anything, through a refusal *strictly stronger* than the verdict (the export menu,
  below). The pair *are* the guarded ones: they call `schemaic_core::sql::run_verdict`
  (pure + tested) and, on anything but `Allow`, park the request
  in `ui.overlay.run_guard` and execute **nothing**. The unguarded actions never leave
  `schemaic-app`; `TabsActions::run_anyway` is the only way back to them, and it replays only what
  the guard parked. The editor pane renders the bar — it does not own the guard.
  **What the guard judges is the statement after parameter substitution**, never the template the
  editor holds. The pair call `params::prepare_run`, which substitutes and *then* asks
  `run_verdict`, and hand back both halves — so the statements that run, the ones parked for "Run
  anyway", and the ones the verdict was formed about are one and the same text. A
  `params::ParamValue::Raw` expands to arbitrary SQL, so a guard shown `SELECT … WHERE x = :p`
  would be reading a statement the engine never receives while `…; DELETE FROM t` is the one it
  does. A parameter with no value stops the run before the guard is consulted at all, as a hard
  hold with no "Run anyway": there is nothing to judge yet.
  This is written down because the guard used to be two closures inside `editor_pane.rs`'s *view
  body*, so it protected exactly one caller: the command palette's `>run` and the AI chat's
  **Insert & Run** both reached the raw action and ran writes past all three protections — the
  missing-`WHERE` net, `confirm_writes`, and the read-only-connection block that by design has no
  "Run anyway". **Don't add a run path that takes the raw action, and don't re-implement the
  verdict** — a new protection is an arm of `run_verdict`, and a `RunVerdict::Block` must stay
  un-overridable. (`plan_view`'s `contains_write` is not a second guard: it decides whether
  `EXPLAIN ANALYZE` may run a statement for its timings.)
  **The two re-run affordances answer to the guard too, through a refusal strictly stronger than
  it.** `ExportScope::AllRows` re-executes the tab's captured statement through `Db::stream_query`,
  and the capped notice's "read N rows" re-executes it through `apply_view` at a bigger ceiling.
  Both are paths executing user SQL that reached the server without passing `run_verdict` at all —
  so an `UPDATE … RETURNING` on a table past the row cap could be run a second time from a Save
  dialog or from a link that says *read*, with no confirmation, on a read-only connection included.
  `sql::rerunnable_for_export` is the gate, built on the same `contains_write` the verdict uses so
  there is one answer to "does this statement write" and one place to grep, and it is **never weaker
  than `run_verdict` and deliberately stronger**: the verdict may say `Confirm`, but neither of these
  has a moment at which to ask — the user picked a file name, or followed a link about reading, and
  a confirmation raised from either would be a question about something they never requested. So it
  is a flat refusal. That `contains_write` is a whitelist of read *heads* is what makes it right
  here: `CALL proc()`, `INSERT … RETURNING` and a data-modifying CTE are all writes to it, and each
  of the three returns rows, so each could otherwise have reached a truncated grid and been offered
  the scope (`a_row_returning_write_is_never_rerunnable_for_an_export`,
  `an_ordinary_read_is_rerunnable_for_an_export`).
  **Running a `.sql` script is the third such path, and it is refused the other way round.**
  `sql::script_verdict` gates `script.rs`'s runner. `run_verdict` takes the statements; a script has
  tens of thousands of them, arriving a block at a time, and *none of them read* at the moment the
  user presses the button — there is no `&[String]` to hand it, and a confirmation raised at
  statement 30,000 would be a question about a file the user can no longer see the start of. So the
  gate is one decision at launch, and it is strictly stronger than the verdict on every axis: a
  script is **unconditionally** a write (the file has not been read, so "does it write?" cannot be
  asked, and the safe answer to an unaskable question is yes), which means a read-only connection is
  refused without opening it and a script of nothing but `SELECT`s is refused too; `no_database`
  blocks outright rather than only when some statement `needs_database`, because an unscoped script
  on PostgreSQL builds into a maintenance database the schema tree can never show again; and it
  **never returns `Allow`**, whatever `confirm_writes` says — that setting is about a statement the
  user typed and can see, and a file picked from a dialog is the case it was written for rather than
  an exception to it. The relation is test-enforced rather than asserted:
  `the_script_gate_is_never_weaker_than_the_run_guard` compares the two verdicts over every policy
  and a set of statement lists reaching each of `run_verdict`'s arms, on a Allow &lt; Confirm &lt;
  Block ordering. Note that this is the *inverse* asymmetry to `rerunnable_for_export` above — that
  one drops the `Confirm` arm because there is no moment to ask, this one keeps only the `Confirm`
  arm — and both are stronger than the verdict, which is the property that matters.
  **What satisfies that `Confirm` is `script_view`'s panel, not a bar**, and the distinction is
  worth stating because getting it wrong is a silent hole rather than a visible one. A typed
  `DELETE` raises "Run anyway" because nothing stood between typing it and running it; a script was
  chosen from a file dialog and is run from a panel that first names the statement counts and, in
  red, how many of them destroy or delete data — `script::is_destructive`, whose net is drawn wide
  (it counts `DELETE`, which no dump writes and a hand-written script does) precisely because that
  sentence *is* the confirmation. `run_script` matches the verdict **exhaustively** so this is a
  decision on the page rather than a fall-through: for one build the call site was an
  `if let RunVerdict::Block(..)`, so a verdict of "ask first" ran the file anyway, and the three
  tests over `script_verdict` all passed because every one of them exercised the function alone —
  the defect living, as ever here, at its composition with the caller.
  **One tested function is asked by everything that re-runs**, and that is the shape of the fix
  rather than a tidy-up. `filter::rerun_statement(base, &GridQuery, dialect)` composes *what* would
  run (`build_query`, or the base verbatim when there is no filter or sort) with *whether it may*
  (`rerunnable_for_export`), so the guard is asked about the string that would actually be
  dispatched and there is no arrangement of base and filter that gets a write past it.
  `GridState::current_statement` is a two-line wrapper over it, and the read-more link's predicate
  (`rerunnable`, the same function as a `bool`), its click and `grid::export_menu` all ask the
  wrapper. Its reads are **tracked** so the predicate can be that function rather than a second
  spelling of it — which it had silently become again, and which is how the kept-result term came to
  be written in two places. Tracking costs the other two callers nothing: both run inside click
  handlers, where there is no effect to subscribe. Before this the export menu ANDed a
  separate `!rerunnable` term of its own — a term that could be deleted with the whole suite still
  green, which is exactly what it was worth — while the read-more link asked only whether `base_sql`
  was `Some`, so a `SELECT` followed by a `DELETE` left the link drawn over the new base and a
  row-returning write could be re-run outright. `grid::export_menu` simply does not offer `All rows`
  when the function says `None`, and the notice draws no link; not offering is the whole enforcement,
  since the scope cannot then be chosen. The composition is where the tests now live
  (`a_row_returning_write_has_no_rerun_statement`, which asserts the refusal with *and* without a
  filter, `a_blank_base_has_no_rerun_statement` — `contains_write("")` is `false`, so a bare "is it a
  write?" gate says yes to an empty base — and
  `an_unfiltered_read_reruns_as_itself_even_when_it_cannot_be_rewritten`, the join-and-CTE property
  the read-more link exists for).
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
  read-only gate, `intel`'s tokenizer, `sql_highlight`, `sqlfmt`, and `users::redact_secrets`, where
  a span ended early leaves the tail of a password hash on screen). Never hand-roll a second
  scanner — five drifting copies was the original bug. **It's dialect-aware:** `skip_noncode`/
  `skip_comment` (and the `sql.rs` helpers built on them — `statement_bounds`/`ranges`/`range`/
  `first_statement`, `statement_bounds_open` (the resumable form the script splitter feeds),
  `executable_statements`/`executable_range`/`executable_at`,
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
  PostgreSQL boundaries are deliberately untouched: MySQL's trigger bodies go behind `DELIMITER`.
  **What Run Everything sends did change, on purpose**, and that is the other half of this
  invariant: a *range* keeps its terminator, because the editor selects and highlights with it, and
  a server must never see the client's `DELIMITER` token — `END$$` lexes as one identifier on MySQL
  and failed every dump carrying a trigger. So the three paths that **execute** go through
  `executable_range` rather than slicing the ranges: `executable_statements` (Run Everything),
  `core::script`'s streaming splitter (the script runner), and `executable_at` (Run Current, which
  was left behind by the first two and shipped a release sending `…END$$`). Anything that
  *executes* uses one of those three; anything that selects, highlights or measures keeps
  `statement_range`. The other half of that
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
- **One connection per operation — except a Manual-mode tab, and a running script.** Every `Db` method opens a fresh
  connection, runs, and disconnects; that statelessness is why a dropped connection is never a
  problem. The *first* exception is manual-transaction mode: a tab set to `TxMode::Manual` pins one
  connection (`schemaic_db::Session`, one `Conn`/`Client` behind a `tokio::Mutex`) for the life of
  its transaction, held in the app's `sessions` map (tab id → `Arc<Session>`). Only the tab's own
  work routes there — run, Run Everything, grid writes, and the post-write re-fetch (which *must*,
  since no other connection can see uncommitted rows). Read-only side channels (schema
  introspection, live-validate `PREPARE`, EXPLAIN, Live Monitor, AI/MCP) stay on fresh connections so
  a long transaction can't block them. Don't add a second connection-caching path; extend `Session`.
  **The second exception is `Db::run_script`**, and it is a genuine one rather than a long call:
  the connection is pinned for the length of the whole file. A script's statements are not
  independent. A dump opens with `SET FOREIGN_KEY_CHECKS = 0` (`dump::fk_guard_sql`), may carry its
  own `BEGIN` … `COMMIT`, and on MySQL switches the terminator around a routine — all *session*
  state, so a fresh connection per statement would apply the guard to a connection already gone and
  then fail the load on the first child row. It is not `Session`: that type's automatic `BEGIN` is
  Manual mode's semantics, and a script's transactions belong to the file. For the same reason it is
  **not `run_ddl`**, which on all three engines takes `&[String]` (the whole plan in memory) and
  wraps it in a transaction of its own — a second `BEGIN` around a file that already carries one is
  not what any of the three engines does. `run_script` therefore wraps nothing, and on SQLite it
  also leaves `PRAGMA foreign_keys` alone, where `run_ddl` turns enforcement off for its rebuild:
  silently lifting the guard for a file that never asked would load rows the database would have
  refused, with no commit of ours to check before. The statements arrive over a bounded channel
  (`SCRIPT_QUEUE`), which is both the backpressure and the progress design — the reader cannot get
  more than a queue ahead, so `script::Splitter::consumed` tracks what the server has applied
  closely enough that the driver reports progress from the file position alone, with no second
  channel.
  **A streamed whole-table export fits this rule rather than bending it.** `Db::stream_query`
  connects, runs and disconnects like every other `Db` method; what is new is only how *long* that
  takes, and the bounded channel is what keeps that bounded in memory too — a server faster than the
  disk waits rather than queueing the table. Nothing is cached and no second connection path is
  added, so a `Db` call that runs for minutes is not evidence the rule has been abandoned.
  **SQLite has no exception at all** — every operation opens its own connection inside
  `spawn_blocking`, which is this invariant rather than a concession to a blocking driver, and
  `Session::open` refuses it (see `core::tx` above for why the pinned form needs its own design).
  In-transaction writes nest under a `SAVEPOINT` (`TxScope`) so the 1-row guard can roll back its own
  batch without ending the user's transaction, and the transaction *state* is the pure, tested
  `schemaic_core::tx::TxState` — engine divergence (PG poisons on error, MySQL implicitly commits on
  DDL) belongs there, not in UI conditionals. **The one thing the session asks the server rather
  than the statement text** is whether a failed statement left the transaction alive: `tx_alive`
  (private to `session.rs`) tries `@@in_transaction` — MariaDB's, exact, no privilege needed, counts
  a read-only transaction — and falls back to a scoped `information_schema.INNODB_TRX` count, which
  both servers have and which needs the `PROCESS` privilege the Server Activity panel already
  assumes. It misses a transaction that has done no InnoDB work, which is harmless here because one
  with nothing in it has nothing to lose. Neither answering yields `None` and the conservative
  reading; PostgreSQL is not asked at all, its DDL being transactional and its failure state already
  exactly `Poisoned`. This is the wire-status answer `implicit_commit`'s own doc names as the thing
  that would replace its keyword guess, so it is asked only where the guess is not good enough.
  `Session::fetch_query` also checks the cancellation token **twice** — before taking the session
  lock and again after — because the lock *is* the wait: a `commit_writes` or the re-fetch after one
  can hold this connection for minutes, and a run queued behind it that the user or the statement
  timeout cancels never reaches the server. That is what makes `StmtOutcome::NotSent` reachable, and
  it also makes the outcome deterministic, since every arm below ends in a `tokio::select!` against
  `cancelled()` and `select!` polls ready branches in **random order** — the same coin flip
  `sqlite`'s `refuse_if_cancelled` documents from a real CI failure.
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
  **"Never in a log" includes a log the environment asked for.** `app::logging`'s
  `log_directives`/`filter_for` append `russh=warn`, `russh_cryptovec=warn` and `russh_util=warn`
  *after* whatever `RUST_LOG` said, because `RUST_LOG` **replaces** the default filter rather than
  adding to it, `tracing-subscriber`'s `tracing-log` bridge brings every dependency's `log` records
  in, and `russh` traces the packet body before encryption — so `RUST_LOG=trace` wrote the SSH
  tunnel password into
  `schemaic.log` in cleartext, in the folder Settings offers an **Open folder** button for. A
  dependency that logs a secret gets a line in `CREDENTIAL_TARGETS`, appended last so the floor
  outranks an equal key; never a `debug!` we hope nobody enables.
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
  **The mirror image is a callback that outlives the scope it reads**, and a window-global menu is
  the usual carrier: an entry on the shared popup channel is an `Rc` closure over a cell's signals,
  and nothing clears that channel when the cell goes away. Two defences and both are needed — the
  control takes its **own** menu down in `on_cleanup` (`widgets::close_picker`, at the anchor
  `open_picker` returned, held in a plain `Cell` because a signal read inside `on_cleanup` is the
  hazard the cleanup exists to prevent), and the actions those entries call open with
  `GridState::alive()`, which is `rs.try_get_untracked().is_some()`. Since `get_untracked` is
  `try_get_untracked().unwrap()`, the failure this prevents is a panic that takes the window and
  every tab's uncommitted edits — not a no-op.
- **Themable colors reach reactive styles as `fn() -> Color`, never a captured `Color`.** A `Color`
  read once at build freezes and won't follow a live theme switch; pass the fn and call it inside
  the `.style(move |s| …)` closure (see `FieldCfg::background`).
- **And so do sizes**: a design token that boxes, indents or spaces text is a `fn() -> f32`/`fn() ->
  f64` reading the interface scale (`theme::font_body()`, `consts::row_h()`, `theme::scaled(…)`),
  never a `const`. Same mechanism, same reason — the `.style` closure that *calls* the metric re-runs
  when the scale changes, and a `const` can never re-evaluate. **This extends to a size passed as an
  argument**: `FieldCfg::font_size` and `highlight_text`'s size are `fn() -> f32`, because a caller
  that resolves `theme::font_body()` at build time freezes the value just as surely as a `const`
  would. If a size ends up inside something that isn't a style closure (a text `Attrs` list, an
  editor `Styling`), pass the fn and read it *there*. **The exception list lives on `consts.rs`'s
  module doc** — extend it there, not here: it is prose, prose is the wrong shape for it, and it has
  been found short three times (a source-gate test holding the exceptions as *data* is the form that
  gets updated when it fails, and is proposed but not written). What it says today is that an
  unscaled length is a hairline, an editor-relative metric (the code font has its own size setting
  and the scale doesn't touch it), the seed for a persisted width the user dragged, an **icon base**,
  or `TERM_FONT_SIZES` (the terminal font, which the scale deliberately doesn't reach either).
  **The air a floating box keeps from a panel edge is no longer on that list**: it is
  `consts::float_inset()`, it scales, and `consts::float_inset_gate` is the source gate — with the
  exception list as *data* — that keeps a twelfth site from inventing a sixth number. That entry
  used to read "`GRID_BAR_INSET` (5.0) and `SELECTION_BAR_INSET` (8.0), and whether those two should
  scale is unsettled", because the honest answer needed the app on screen at 160% and no test can
  see a gap. Looked at: the 10–12px gaps read correctly there and the 5–7px ones read tight against
  surroundings that had all grown, so a base of 8 (→ 13 at 160%) replaced all five numbers. A
  *second, gentler curve* was the other candidate — it would have kept 100% pixel-identical — and it
  was rejected because `scale_at` is the only curve in the app (`scale_font_at` delegates rather than
  being a second rounding rule) and a third category would make every future length a taste decision
  between "scales", "doesn't" and "scales a bit". Five numbers for one gap is what that costs. The
  trade taken instead is that 100% moves: the grid's bars loosen from 5, the ER diagram's tighten
  from 10 and 12, and the selection summary — already 8 — is unmoved. The icon bases are the
  two-way trap: `icons::icon(markup, size)` takes a **base** size and scales it itself, so never hand
  it an already-scaled value, which is what `consts::COMPLETION_ICON_BASE` and
  `consts::SCHEMA_ICON_BASE` exist to spell out — and scaling the base constant too would square the
  factor. One more exception is a *shape* rather than a name, and no textual sweep can find it: a
  `border_radius` that is **half a scaled box** is a circle or a pill, not a shape constant, so it
  has to move with the box — a `scaled(8.0)` dot with a fixed 4px radius is a rounded square at 160%,
  in a list where the dot is the only colour cue. Five sites crate-wide, and all five now derive the
  radius from the box they round: the connection colour dot (`dot / 2.0`), the jump-to-bottom circle
  in `widgets.rs`, `properties.rs`'s `bar_h() / 2.0`, `connection_form`'s colour swatches
  (`box_w / 2.0` over a `scaled(18.0)` box) and `settings::themed_toggle`'s track
  (`track_h / 2.0`) — the last two were a literal `9.0`, which is a rounded square at 160%. It is
  *not* the `SEGMENT_RADIUS` case (a shape inside a box that scales), and grepping for an unwrapped
  literal cannot tell the two apart.
- **Pure logic lives in `schemaic-core` with unit tests** — SQL boundaries, edit-model analysis,
  export (incl. CSV formula-injection guard), diff, DDL. The UI keeps thin wrappers.
  **One function in the crate breaks the no-filesystem rule its tests otherwise keep, and it is a
  deliberate trade**: `export::export_xlsx_chunks` drives `rust_xlsxwriter` in `constant_memory`
  mode, which spills each finished row's XML to a library-managed temp file so an export costs a
  bounded buffer at any row count. The alternative holds the whole workbook in memory — 10M cell
  structs at 200k × 50, before a byte is compressed — which is exactly the size the streaming path
  exists for, so the bound is worth more than the purity. The file is created and removed inside the
  call, so a test of it is still deterministic and still needs no server; it is the same pragmatic
  exception in-memory SQLite already is in `schemaic-db`. Read it as the one case, not as licence
  for a second: anything else wanting a file still models it at the boundary.
- **Generated DDL is never run silently, and never emitted from a second differ.** Every
  schema edit goes `TableDraft`/`ViewDraft` → `ddl::diff`/`diff_view` → `ChangeSet::emit` →
  the preview modal → `Db::run_ddl`. **`Db::run_server_ddl` is on the same rule, not beside it**:
  `CREATE`/`DROP DATABASE` reach it through the same preview, from `ddl::server_level` →
  `ChangeSet::emit` → `ddl_preview::preview_container`, and it is a second *runner* rather than a
  second emitter — the two statements can take neither of `run_ddl`'s commitments (see it above).
  Don't add a path that builds `ALTER`/`CREATE`/`DROP` text somewhere else, and
  don't add one that applies a plan without the preview — the preview is where the destructive
  consequence is stated in plain language and where "Open in editor" hands the script over.
  **A dump is on the written side of this rule rather than an exception to it.** `core::dump::plan`
  composes a file out of the emitters that already exist — `TableInfo::create_ddl`, `ddl::view_ddl`,
  `TriggerInfo::create_sql` and `ChangeSet::emit` for the closing foreign keys — and `app::dump`
  writes it to disk; nothing on that path can execute it, which is exactly the standing Copy DDL has.
  A second emitter would be the regression here, not the missing preview.
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
  the DDL preview's Apply, Server Activity's kill and the Export modal's own launch are the four,
  and they go through
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
- **No floem `Dropdown` — every `<select>` in the app drops the app's own menu.** A control that
  offers a fixed list is built with `settings::in_ring_picker` (or one of its two thin wrappers,
  `focusable_dropdown` and `table_designer::focusable_owned_dropdown`); nothing constructs a
  `floem::views::dropdown::Dropdown`, and `settings::no_floem_dropdown_gate` scans the crate's own
  source — comments stripped, so the paragraphs that name the type in order to forbid it don't trip
  it — to keep that true. The reason is the paint-only overlay nudge under *Floem 0.2 gotchas*:
  floem places its dropdown's popup as an overlay and, when that popup would overflow the window,
  shifts it back inside **during paint only**, leaving layout and hit-testing where they were. Every
  dropdown in the app carried that latently, and the interface scale is what made it reachable — a
  `modal_h(620)` editor is 992px tall at 160%, so a field near its foot is within a popup's height
  of the window edge on any 1080p screen. There is no bounded fix inside floem's control: the nudge
  is `cx.offset((-x, -y))` against `parent_size - 5.0`, so a `max_height` cap would have to be ≤ 5px
  to guarantee no nudge at all. The class is therefore removed rather than its instances — which is
  cheap only because the app already owned the machinery: `widgets::menu_panel` flips at an edge
  from a predicted height (`menu_panel_height`) rather than trusting an overlay to fit, and
  `widgets::{PopupChannel, open_picker, close_picker}` is the trigger half the row panel's pickers
  pioneered. **Don't reintroduce it for "just one more" control**, and don't reach for a floem popup
  primitive without checking it against that gotcha first.
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
  `db::ident_sqlite`, `users::pg_ident` and
  `db::ident` are all thin delegations; the three engine-fixed ones in `schemaic-db` are bound by a
  test in that crate, since they can't take a dialect. **Don't write a fifth** — there were four,
  each having independently arrived at the same escaping, which is the drift hazard rather than the
  reassurance: the literal half of the same split (`schema::ddl_string` missing MySQL's backslash
  escaping while `export::sql_literal` had it) shipped as a High.
  **A MySQL account is not an identifier**, which is the one case that looks like it wants a fifth
  and doesn't: `'app'@'%'` is two *string literals*, so `users::account_sql` goes through the literal
  quoter (`schema::ddl_string`) and only PostgreSQL's host-less half reaches `export::ident_sql`.
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
  **Snippets are a fourth category, appended after all three** (`overlays::snippet_items`), with the
  bookmark glyph the panel toggle wears — in the **accent**, the only row colour in the list, because
  a snippet is the one result that is the user's rather than the server's (the completion popup tints
  its snippet rows for the same reason). They
  sit outside that cap because they come from `snippets.json` rather than from the schema passes:
  the palette is a way to reach the database, and a saved query is a thing you wrote *about* it.
  Activating one inserts it at the caret, as its library row does — and it is **not recorded in the
  search history**, which is what lets it be here at all: a `SearchEntry` resolves against the live
  catalogue, and a snippet is not a catalogue object, so remembering one would mean teaching the
  persisted `ObjectTag` about a thing no schema can confirm (and `snippet::next_id` reuses a
  deleted id, so a remembered one could later name a different query).
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
  **Cut where an invariant is, not where the line count is.** `modals.rs` is the worked example:
  the modal layer was not the largest thing in `lib.rs`, it was the thing whose *tuple order was
  policy* and whose gate could then sit beside what it guards. Two things make such a cut checkable
  in a diff nobody can read line by line — and this is a move whose whole risk is that. First,
  **strip comments and blank lines from both sides and diff the code**; a move that is right shows
  only the new scaffolding (imports, the signature, the bindings) and the handful of edits you
  meant. Second, **prove the moved tests still bite in their new home**: a source-scanning gate that
  reads a file by name passes vacuously the moment the name is wrong, so break a term, watch it
  fail, and put it back. Both were done for this cut, and the second is the one that would have
  caught a gate left pointing at `lib.rs`.

## UI conventions

- **No pointer cursor on buttons/icons** — native apps keep the arrow cursor; a pointer feels
  web-like. Use the default; reserve `CursorStyle::Text` for text inputs (a genuine hyperlink may
  keep `Pointer`). The account browser's rows shipped with `CursorStyle::Pointer` and are named here
  because a rule with no instance reads as a preference: after that one was removed the only
  `CursorStyle::Pointer` left in `schemaic-ui` is the terminal's hyperlink run in `lib.rs`, which is
  the exception the bracket allows. One `grep` is the whole check.
- **A selection outline is painted in both states; only its colour changes.** Taffy sizes the
  **border box**, so a 1px rule added only while a control is picked grows it 2px: a button sized by
  its own padding jumped a pixel each way the moment it was chosen and the row re-flowed around it,
  which is how every toggle in the two account forms shipped.
  `account_editor::picked_outline(style, picked)` is the one helper, and it paints the border
  always, `theme::accent()` when picked and `Color::TRANSPARENT` when not, which leaves
  the colour as the only thing that moves. Its remaining caller is the grant form's `privilege_tag`
  — Kind, Level and the two option rows are now the app's `<select>` and its switch — and that
  wrapping tag cloud is the reason the helper is still here: the tags **wrap**, so a tag that grew
  as it was picked would re-flow every tag after it on the line, not just itself.
  This is the same accounting `widgets::row_menu_mark_pad`
  does for a tree row (*Popup menus*), taken from the other end — that row's height is fixed by what
  surrounds it, so the 2px is given back out of its padding, while a button sized by its own padding
  has nothing to give back and needs the border there all along.
- **`btn_primary_text` is not "white on the accent".** It is the *label* colour of the Primary
  button, chosen against `btn_primary`'s own dark navy fill — `#8EA7EA`, a light blue. Painting it
  on a saturated fill leaves a glyph nobody can see, which is exactly how the import list's
  checkbox shipped its first draft: a filled blue square with an invisible tick in it. The pair
  that *does* mean "on, and read me against it" is `toggle_on` + `toggle_handle_on`, the second of
  which is `#FFFFFF` in both themes precisely because it is drawn on the first — so a new filled
  control that has to show something inside itself takes the toggle's colours, not the accent's.
  Small glyphs drawn inside one also want `icons::CHECK_BOLD` over `icons::CHECK`: Lucide's
  stroke-width 2 is tuned for ~16px and falls under a device pixel at 11. The result is
  `widgets::check_box`, and it is **the** checkbox: a multi-select list draws that, never a box of
  its own, which is what the dump modal's table picker was until it adopted it.
- **Menu labels carry no trailing ellipsis**, even when the entry opens a dialog. The platform
  convention says a `…` means "this will ask you something first", but this app doesn't keep it:
  of the ~110 menu labels in `schemaic-ui`, the only three that ever had one were added in a single
  sitting and removed in the next. Follow the count, not the platform guideline — a menu where
  three entries out of a hundred trail dots reads as an inconsistency, which is what it is. Written
  down here because it was unwritten, and an unwritten convention costs a review round every time
  somebody adds a menu.
- **The footer's panel toggles: a control that can't act must not look like one, and when the user
  can fix that, say so.** Below their breakpoints the side panels are *force-hidden* — the schema
  tree at `panels_min_schema_w`, the right column at the wider `panels_min_full_w` — and the window
  narrowing past one is something the user does by dragging an edge, not a rare state. Every guard
  for it was already in place (`schema_panel_allowed`/`right_panel_allowed`, read by `body`, the
  dividers and each toggle's `active`), so the toggles were *correct* and silently did nothing: five
  status-bar icons at full brightness whose clicks changed a signal nothing renders, which reads as
  a broken button rather than as a consequence of the window size. They now wear
  `toggle_icon`'s disabled face and carry a tooltip saying to widen the window.
  **`panel_toggle(fits, offered)` answers the face and the tooltip in one call**, and that is the
  point of it rather than a convenience: they are the same fact, so two predicates could disagree in
  two bad ways — a dimmed toggle explaining nothing, and a live toggle insisting the window is too
  narrow for it. Unit-tested over every input, including the invariant that a tip never accompanies
  an enabled toggle. **Only the narrow case gets a tip**, deliberately: it is transient and
  actionable, while Server Activity on an engine with no sessions is permanent and keeps
  `toggle_icon`'s original ruling that *a toggle which opens an explanation is a worse answer than
  one that visibly isn't offered*. When both apply the narrow one speaks, being the half the user
  can act on.
  **Find-Anywhere's Toggle Panel rows answer the same state, in the palette's own idiom:** dimmed
  and skipped, with no tooltip — a list row is not a control with a hover affordance, and the rows
  beside it are the explanation. They read the same two predicates, so the palette and the status
  bar can never offer different answers. Note the palette now has **two** answers to "unavailable"
  and they are not interchangeable: *omission* for Server Activity on an engine with no sessions,
  because that panel does not exist for the connection and a row would be a route the mouse
  refuses; *dimming* for a window that is too narrow, because the command is real and will work
  again in a moment — and dropping those rows would make the list's length jitter while the user
  drags a window edge. `PaletteItem::enabled` carries it, `widgets::list_step_enabled` and
  `first_enabled` keep the arrow keys off dead rows (unit-tested, including a cursor left on a row
  that has just gone dead), and `open_sel` refuses one that Enter still reaches.
- **A tooltip that appears only sometimes needs `widgets::tip_when`, not a branch.** Floem's
  `.tooltip()` has no "not now": once the hover delay fires it always adds the overlay, and an empty
  tip is a small empty box because `TooltipClass` paints its chrome on whatever root it is handed.
  The two older conditional tips (a truncated ERD header, a tab's path) decide **once, at build**,
  and an `AnyView` branch is right for them. A condition that changes while the app runs — a window
  width — cannot use that shape, and does not need to: floem calls the tip closure at the moment the
  delay fires, so a signal read there is read fresh on every hover with no rebuild underneath. What
  it returns when there is nothing to say is a `display: none` root, which the chrome cannot
  override — floem hands each `.style()` closure a fresh `Style` and merges results per property by
  push order, and `tooltip_style` sets no `display`.
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
- **Theming (`themes.rs`)**: three independent axes — `UiTheme` (chrome: dark/light), `EditorTheme`
  (editor surface + syntax tokens: One Dark Pro / Tokyo Night / Catppuccin Latte) and `UiScale`
  (how large the chrome is drawn). A theme is a flat struct of named colour roles (hex). All three
  live in `Scope`-owned global `RwSignal`s; `theme::set_ui`/`set_editor`/`set_ui_scale` swap them.
  The choices are persisted (`ui_theme`/`editor_theme`/`ui_scale` in `UiState`) and seeded via
  `theme::init` before the view builds. Editor tokens re-highlight on switch because
  `SqlStyling::id()` returns `theme::editor_generation()`.
  - **Live-switch caveat**: a colour read *inside* a reactive `.style` closure updates instantly; one
    captured *by value* freezes at build time. Prefer `fn() -> Color` for anything themable (see
    `FieldCfg::background`).
- **Interface scale (`UiScale`: 80% / 100% / 130% / 160%)** multiplies the
  *design tokens*, not the window. Floem 0.2 has no user-settable render scale, and a paint
  transform over the whole tree would take hit-testing and the editor's own coordinate arithmetic
  with it — so instead the type scale (`theme::font_*()`), the layout metrics (`consts::*`), the
  per-module modal metrics, `icons::icon` and every `padding`/`gap`/`margin` in the views (~600 call
  sites, each wrapped as `theme::scaled(…)` *inside* its style closure) all read the signal and round
  to whole pixels (`themes::scale_at`, unit-tested: **`Normal` is the exact identity**, results are
  integral, and a positive size never rounds to nothing). The air between things grows with the
  things, which is what stops a 160% window reading as big type crammed into 100% boxes.
  - **What it deliberately doesn't touch**: the SQL editor's code font and the terminal font, both of
    which already have their own size setting (and the editor a per-tab Ctrl+scroll zoom whose
    override is an absolute px that a second multiplication would double-apply); persisted panel
    widths, which are px the user dragged — the *minimums* they clamp against scale, which is what
    keeps a panel from being narrower than its own text; and the ER diagram's canvas, which has its
    own zoom.
  - **Shapes and hairlines stay literal**, which is the line the padding sweep stopped at. A
    `border_radius` is a *shape*, not air — scaling a 5px corner to 8px reads as a different design
    rather than a larger one — and a 1px rule or border is a hairline at every scale, so `.height(1)`,
    the menu separator's rule, the menu panel's border and `edit_field`'s `+ 3.0` (two borders plus a
    pixel of rounding slack) are all deliberately unwrapped. `menu_panel_height` is where the
    distinction is visible in arithmetic: its boxes scale, its two borders don't. **The exception is
    a radius that is half a scaled box** — a circle or a pill, which stops being one the moment the
    box grows and the radius doesn't; see the interface-scale invariant for the five sites and for
    why no grep finds them.
  - **A modal's size scales, then caps against the window** — `widgets::modal_w` for the width,
    `modal_h` for a fixed-height panel, `modal_body_h` for a scrolling body inside one. All three
    read `window_size()` inside the caller's style closure, so a resize re-runs them, and all three
    clamp their own floor to the window (a *scaled* floor passes 500px, and one wider than the
    screen would clip through the guard meant to keep the panel usable). **On the vertical axis the
    extent is the modal *layer*, not the window** (`cap_to_window` measures `modal_layer_h()` — the
    window less `header_h()`): since `07bda98` every modal hangs in a layer inset by exactly the
    `scaled(40.0)` that is also `modal_h`'s reserve, so measured against the window the layer spent
    the guard's whole budget before the panel was sized. On a roomy window the panel came out
    *exactly* as tall as its container, flush under the title bar and flush against the window's
    bottom; on a short one the floor — clamped to the window — made it taller than the container with
    nothing to clip it back, putting the footer where Apply lives off the bottom edge. Only the
    height needs the inset, the layer being full width
    (`a_modal_fits_the_layer_it_is_centred_in_not_the_window`; three existing tests had asserted the
    pre-`07bda98` geometry and were corrected with it).
    - Heights were left unscaled at first, on the grounds that 620px is already most of a laptop
      screen — but the type inside them grew, so at the 200% then offered an editor was three
      fields and a scrollbar.
    - **Width is the one that must fit first.** A modal is centred in a full-window backdrop: one
      taller than the window loses its footer (where Apply lives) off the bottom, but one *wider*
      than the window loses its left half — the designer's list pane and every field label with it.
      The 900px editors came to 1800 at the 200% then offered, wider than a 1440p screen's window; at
      today's 160% top scale they come to 1440, which a 1366px laptop still cannot hold.
  - **A size the scale has to reach cannot be a plain `f32` parameter.** Three carry a `fn() -> f32`
    for exactly the reason the colours do:
    - `FieldCfg::font_size` — `edit_field` derives its box height, padding, placeholder position
      **and** its editor's `FieldStyling` from it (a hand-written `Styling`, because floem's
      `SimpleStyling` takes the size by value);
    - `widgets::highlight_text` / `highlight_mono` — their `rich_text` closure *is* reactive, but
      only for what it reads inside itself;
    - `widgets::loading_dots` / `verb_spinner` — which also *measure* from it (the reserved width
      that stops the dots reflowing), and which live for as long as the operation they report.

    All three shipped as `f32` and all three froze at the size their view was built at: every text
    field kept 13px type in a box grown for 20px, every highlighted row in the tree / history /
    activity panels kept its old size until a filter change rebuilt it, and the app's one moving
    indicator stayed at whatever scale was active when the query started.
  - **`set_ui_scale` bumps `ui_generation`** for the one place a size still can't be re-read: a text
    `Attrs` list built outside a reactive closure (`markdown.rs`). Views keyed on the generation
    rebuild.
  - **A *structural* decision taken against a scaled measurement has to be in the container's
    rebuild key**, because it is the one kind of answer a `.style` closure cannot give: whether to
    attach a tooltip at all, or how wide a scroll range is, is not a property that re-runs.
    `editor_pane`'s Ctrl+K / inline-AI key carries `theme::ui_generation()` because `diff_view`
    measured `content_w` from the font at build time and baked the diff's syntax colouring into a
    text `Attrs` list — without it a live scale change left the rows rendering at 1.6× inside a
    `min_width` computed at 100%, so the end of a long line could not be scrolled to. **Both of
    those measurements went with `diff_view`** when the suggestion moved into the editor's line flow:
    the rows are laid out by the editor at the *editor's* own font (which the interface scale does
    not touch) and their colours are resolved inside `inline_diff::segments` on every call. What is
    left in that container is a bar and a footer whose every metric is read inside a style closure
    (`theme::scaled(…)`, `icons::icon`'s own closure, `FieldCfg`'s `fn() -> f32` sizes), so **the
    key was dropped with the measurements** — read the Ctrl+K story as the case that *taught* this
    rule rather than as a live instance of it. `tabs`' chip
    key carries `theme::ui_scale()` **and the presence of the DB-identity dot**, for the same reason
    from the other side: `truncated` is computed once at build and decides whether the chip gets a
    tooltip, and all three terms of that comparison move — the measured title, `tab_title_avail()`,
    and the `tab_dot_w()` the dot sheds from it — so assigning a colour to the open tab's database
    narrowed the title enough to ellipsize it while `truncated` still said it fitted, and the one cue
    that says what the clipped title is never arrived. The dot is read there as a *presence*, not a
    colour: the swatch is an ordinary style read and only the width it costs is structural.
  - **A cursor menu's flipped arm pins a *trailing* inset; it never computes a leading one**
    (`widgets::cursor_menu_insets` → `MenuInset::Start`/`End`). Four arms per axis: after the cursor
    if it fits, before it if that fits, flush with the window's far edge when neither does, and the
    window origin for a panel bigger than the window. The panel's size is an **estimate**
    (`menu_panel_height` counts rows), so it is allowed to decide *which* edge and nothing else —
    subtracting it from the cursor put the real edge wherever the estimate was wrong, which showed
    as a gap between a flipped menu's bottom and the pointer that flipped it. `submenu_insets` has
    always worked this way; this is the same trick, and the reason that one never drifted.
    - **A panel dropped from a *box* flips above the box's top, not above the point it drops from**
      (`widgets::box_menu_inset`). A cursor is a point, so `menu_inset`'s single anchor is right for
      it; a control has two edges, and reusing the bottom one for the flipped arm puts the panel
      across the control — which, when the control is the button that opened *and* closes the panel,
      means the panel covers its own dismissal. Two callers: the date field's calendar
      (`calendar_insets`) and every menu anchored `PopupAnchor::BelowBox` — the row panel's enum,
      boolean and set pickers, and an open grid cell's. Both anchors therefore carry the control's
      **top** edge as well as its bottom. `BelowIcon` does not and stays on `menu_inset`: its panel
      is meant to overlap its 28px glyph, which is the whole difference between the two variants.
    - Two earlier spellings both failed at scale, and both are worth not re-deriving: clamping the
      flipped arm at zero threw every context menu to the window's top-left corner once a menu was
      600–750px tall, and scaling the whole 30.5px row estimate (padding included, back when the
      paddings were still literal) over-predicted a long menu by ~190px and flipped menus that had
      room below them. The estimate is still summed from its parts now that the paddings do scale,
      for a smaller but live reason: each part rounds to its own whole pixel the way the style that
      draws it does, and the panel's two 1px borders must *not* grow with the boxes.
  - **The interface scale is a segmented control, not a dropdown** (`settings::scale_picker`),
    because floem nudges an overflowing overlay in paint only — see *Floem 0.2 gotchas*, where the
    finding is written up in full. Four short segments, one Tab stop with Left/Right inside it
    (`nav_group`), arrows applying as they move. The segments are labelled with the **percentage
    itself** (`UiScale::label`) rather than Small/Normal/Large/Huge: the number is what the control
    is asked about, four of them fit the row at every scale where four names didn't, and a label
    computed against `factor()` can't go stale when a factor is retuned — which is why the variant
    names (internal only) and the persisted `key()`s (semantic, so a retune carries a stored choice
    rather than resetting it) are deliberately *not* the same vocabulary.
  - **The AI panel's height floor is held only while a turn streams** (`next_floor(…, !busy)`).
    `ai_panel`'s `floor` exists for the measurement dip a rebuilt `RichText` reports mid-stream; an
    idle panel has no dip to hide, so it takes what it just measured. It *also* releases early,
    inside a stream, on the things known to change the true height (message count, wrap width,
    connection, interface scale) — but that list is an optimisation now, not the guarantee, because
    it was incomplete twice: first the wrap width, then the scale, each leaving a **permanent** band
    of blank under the last message rather than one frame of it. Written as the premise, a missed
    invalidator closes itself on the next layout.
  - Derived metrics sum the **scaled parts** rather than scaling the sum (`consts::leaf_pad`), so an
    indent lands on the pixel the glyphs it aligns under actually occupy — at 80% those differ by one.
- **Reactive text**: use `dyn_container` (no `floem::views::label`).
- **There is no accessibility tree, and there is nothing in this repository that can add one.**
  Floem 0.2.0 ships no AccessKit integration and no a11y surface of any kind — grepping the crate
  for `accesskit`/`accessibility`/`a11y` turns up only the English word in two doc comments about
  platform config — so nothing the app builds is exposed to Narrator, VoiceOver or Orca, whatever
  it is labelled. What the app *does* have is keyboard operability and a legible size, and those are
  the axes worth spending on: `FocusRing`, `focus_root_with_ring`, spaced tab indices,
  `widgets::accept_launch`, the `shortcuts.rs` table with the test that fails when a binding has no
  row — and the **interface scale** (`UiScale`), which is why the chrome's type is a set of
  functions rather than a 13px constant. The README says this out loud under *Accessibility* rather
  than leaving someone to discover it after the download; revisit if a later Floem grows the layer.

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
- **`.tooltip()` wraps the view; it does not decorate it.** `tooltip(child, tip)` builds a *new*
  view and makes the old one its child, so everything chained after it lands on the wrapper and
  everything before it stays on the inner view. Split a control across that line and it comes apart:
  the style — background, border, padding, height — paints the wrapper while the listeners sit on a
  content-sized box inside it. **The results strip shipped exactly that**: chip styled after
  `.tooltip()`, click handlers before, so the chip you saw was the wrapper and the chip that
  answered a click was the text, and clicking the padding did nothing. `activity_panel`'s clock
  button was bitten from the other side — `on_move`/`on_resize` on the inner container reported a
  bare 16px glyph box, and its menu hung 3px under that instead of under the padded control.
  So: **style and listeners on the same view, and put the tooltip on the innermost thing that
  should carry it** (the query strip and the results strip both tooltip their *label*), or chain
  `.style()` before `.tooltip()` when the wrapper is genuinely what you want to place.
- **A view must not subscribe to a signal that changes as part of unmounting it** — the change and
  the teardown land in the same update pass, and whichever order they run in, a nested
  `dyn_container` inside the doomed view rebuilds a child whose style/effect closures then read
  signals whose scope has already been disposed. The symptom is floem's own
  `Option::unwrap()` on a `None` in `floem_reactive::read`, with the panic pointing at whatever
  signal the *new* child happened to read first — never at the subscription that caused it.
  The results strip shipped this: the grid's "is this result pinned?" flag was a memo over
  *whichever panel is shown*, so running a query while a result was pinned changed it for the grid
  being torn down, re-ran that grid's edit-model effect, rebuilt the toolbar's `ai_menu` and read a
  disposed `GridState` signal. The fix is not to defer or guard the read — it is to scope the
  subscription to a fact about the view itself (`Tab::panel_frozen_memo(id)`, one panel), and to
  have a departing entity keep its last answer rather than report a "gone" value on the way out.
  The neighbouring rule (**show and hide with `.hide()`, never a rebuild**) is a defence against the
  same hazard: a view that is never rebuilt has no new child to read a dead signal.
- **A view's own `event_before_children` runs *before* its listeners, and a processed event stops
  there** (`context.rs::unconditional_view_event`) — so `on_event(KeyDown, …)` on a built-in view
  never sees a key that view handles itself. **`floem::views::text_input` handles Escape by calling
  `app_state.clear_focus()`**, which is how the grid's inline cell editor came to leave the keyboard
  on *nothing*: its own Escape arm was dead code, and after a press the arrows, Enter and Ctrl+Enter
  all did nothing until a cell was clicked. There is no `disable_default_event` escape hatch worth
  using either — it is per *listener kind*, so suppressing KeyDown suppresses typing.
  What the host does hear is `FocusLost`, which is also what a click on anything else produces, and
  floem exposes no way to ask where the focus went (`AppState::focus` is private, and by the time a
  listener runs `focus_changed` has already assigned the new one). `grid::reclaim_keyboard` is the
  compromise: the **pointer** decides, and it is over the grid for Escape and over the something-else
  for a click on it. `edit_field` does not have the problem — it is a `text_editor` with a command
  hook, so its `on_escape` is real.
- **An overlay that would overflow the window is nudged back in *paint only*.**
  `OverlayView::paint` (`floem-0.2.0/src/window_handle.rs`) does
  `cx.offset((-x, -y))` when `window_origin + size` passes `parent_size - 5` on either axis — and
  nothing corresponding happens to its layout, so hit-testing keeps answering at the *un-nudged*
  position. Everything inside such an overlay is drawn in one place and clicked in another, by
  however much it overflowed.
  - This reaches the app through **`floem::views::dropdown::Dropdown`**, whose popup is an overlay
    opened at `box_origin + box_height`: a dropdown low enough that its list runs past the window's
    bottom paints its rows shifted up while the pointer still lands on the row *below* the one you
    see. Every dropdown in the app could hit it; the interface-scale picker did first, because it
    was the last row of the last group of the tallest modal — and only at 150% (130% now), the one
    scale where that modal grows enough to push the popup off the bottom but not enough for its body
    to scroll instead. Two rounds of plausible fixes (rebuilding the control, chasing a stale
    `window_origin`) changed nothing, because nothing on our side was wrong.
  - **So nothing in this app uses `floem::views::dropdown::Dropdown` any more** — see
    **No floem `Dropdown`** under *Architecture invariants*. There is no bounded fix inside floem's
    control (a `max_height` cap would have to be ≤ 5px to guarantee no nudge), so the class was
    removed rather than the instances: every `<select>`-shaped control drops a `widgets::menu_panel`
    through `settings::in_ring_picker`, and that panel is placed by the app's own arithmetic
    (`menu_panel_height` predicts, `box_menu_inset` flips) and laid out where it is drawn. The
    interface scale stays a segmented control (`settings::scale_picker`) because it never needed a
    popup, not because dropdowns are still unsafe.
- **Enter and Space arrive at a focused view as a synthesised `Click`, *before* its KeyDown
  listeners run — and `Stop` cannot call it off.** `EventCx::unconditional_view_event`
  (`floem-0.2.0/src/context.rs`) applies `EventListener::Click` on the focused view for any
  `is_keyboard_trigger()` key (Enter, NumpadEnter, Space — matched on the **physical** key), and it
  does so in the `match &event` block *above* the generic listener dispatch. Worse, that dispatch is
  `handlers.iter().fold(false, |handled, h| handled | h(&event).is_processed())` — every handler
  runs and only then is the result reported, so one returning `EventPropagation::Stop` neither skips
  its siblings nor unwinds the click that already fired.
  - So a control with both an `on_click` and an `on_event(KeyDown, …)` gets **two** calls for one
    Enter. That is harmless when the action is idempotent (`open.set(true)`, which is what the old
    floem-`Dropdown` wrapper did and why nobody noticed) and a live bug the moment it **toggles**:
    `settings::in_ring_picker` opens through `widgets::open_picker`, which closes a menu already
    standing, so claiming Enter there would open and shut the menu in one keystroke, which looks
    exactly like a control that ignores the keyboard. It claims only Up/Down, which are not
    keyboard triggers and so have no click to collide with, and lets the synthesised click carry
    Enter and Space.
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
- **An absolute child resolves against its direct parent, and that is a lever, not just a hazard.**
  Floem does not look for a nearest *positioned* ancestor the way CSS does: an
  `absolute().inset(0)` child fills whichever box its parent happens to be. Every modal backdrop in
  the app is written that way, so what they cover is decided entirely by the wrapper they are
  mounted in — and `modals::modal_layer` uses that to hold all of them in **one modal layer inset
  `HEADER_H` from the top** (*all*: the membership test is "does this view paint
  `modal_backdrop()`", which is why the Find Anywhere palette is in there beside the DDL editors),
  which is what leaves the title bar free for
  `window_chrome::over_backdrop`'s drag band. The alternative, hoisting the band above a
  full-window layer, would have put it on top of whatever the modal had in that strip: a 620px
  panel centred in a 700px window reaches into the top 40px, and its close × would have been
  answering to the window's caption buttons. **Which is the same trap the band then fell into one
  layer up**: mounted outside the root it was above the *overlay* menus too, and the ones that open
  from inside a modal reach into that strip whenever they are tall enough to pin at y=0. Painting
  later is also being hit first, so "what must this out-paint?" and "what must this be hit before?"
  are one question in Floem, and the band's answer to both is the header and the backdrop. The same lever has a matching trap — a wrapper that
  is *zero-sized* (the out-of-flow state every one of these overlays takes when closed) gives its
  absolute children a zero box, so a modal left out of the layer's `modal_backdrop_up` predicate
  does not open half-right, it does not open at all. That is deliberate: the loud failure is the
  guard that keeps the layer and the predicate in step — and it is only loud to whoever *opens*
  that modal, which is not the person adding it. **It also only covers one direction**, which is why
  the claim no longer rests on it alone: a surface that paints a backdrop and is mounted *outside*
  the layer resolves its `inset(0)` against the root, looks perfect, and silently restores the exact
  bug the layer was written to fix — the backdrop over the title bar, the drag band never rising, and
  a window that cannot be moved, minimised or closed with nothing on screen saying why. Three of the
  layer's members are loose children with no group wrapper to remind anyone, so that is the shape the
  next overlay will take. `modals::modal_backdrop_gate` asserts **which files** paint one, with
  `window_chrome.rs` as the documented exception (`over_backdrop` paints the same scrim across the
  title bar while a modal is up and is mounted in the workspace root, after the layer and before the
  overlay menus). Only `theme.rs`/`themes.rs` are skipped, as the colour's own definition:
  **`lib.rs` came off that skip-list when the layer moved out of it**, since it was there for prose
  the scan already strips and it was blinding the gate to the file holding the root tuple — the one
  place a stray modal would most plausibly be mounted as a *sibling* of the layer rather than inside
  it. It is deliberately weak in the way its three siblings are — files, not counts,
  because a count fails on an innocent refactor and a gate that cries wolf gets deleted, while the
  failure worth catching is a backdrop appearing somewhere *new*, and a new place is nearly always a
  new file. A second test pins the other direction on the *returned closure* rather than the
  function — binding a predicate and then not `||`-ing it in is precisely the mistake, and it leaves
  the binding's name in the body, so scanning the function would pass — asserting the terms are in
  the answer: the three grouped predicates (`ddl`/`workspace`/`settings`) and the three open
  signals. A group added without joining it fails there instead of opening into a zero-sized box.
  **That gate's own list has to be kept current, and currently is not**: the shared confirm became
  the layer's last entry and took a seventh term in `modal_backdrop_up`, but the array in the test
  still names six, so that term is the one thing here nothing pins. **The event editor shipped absent from the
  predicate** and painted nothing at all, so the schema-editing half of that list is now
  `ddl_editors_up(DdlUi)`, split out of `ddl_modals_up(&Ui)` for the one reason that matters here:
  it can be tested. `ddl_preview::tests::every_editor_raises_the_group_that_gives_it_a_box` raises
  each editor target **alone** and asserts the group sees it, over the same `DdlUi` fixture
  `close_editors_clears_every_editor` already builds. The two tests are one invariant read from
  opposite ends — every editor must be in this list, and every editor must be cleared by that one.
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
  That same formula has a sharp edge, because the parent's **border** is *added* to the inset where
  its padding is not (`start + constants.border.cross_start + margin`): a border that comes and goes
  moves a start-anchored absolute child by its width, even though the parent's own height never
  changes. The schema tree's 1px open-menu rule (`menu_mark`) shifted `size_badge` down 1px on every
  right-click that way, while the row's flex children sat still. Anchor to an edge only if no border
  will ever appear on it; otherwise centre — `align_self(AlignItems::Center)` takes taffy's other
  branch, which subtracts `content_box_inset` at both ends and cancels a symmetric rule.
  What you give up is the flow's collision handling: the in-flow sibling now runs *under* the
  overlay instead of pushing it along — in paint order and in the hit test both, so the overlay
  also needs the `.pointer_events(|| false)` the absolute-overlays bullet above is about.
- **Deferred layout**: `exec_after(Duration::ZERO, …)` runs after layout settles — so
  `scroll_to(bottom)` clamps against new content height, not stale.
- **`viewport` announces a width change; the editor's *lines* are not rebuilt yet.** Floem reacts to
  `ed.viewport` in an effect of its own — `lines.set_wrap(Width(viewport.width()))` — which only
  **clears** the line layouts, rebuilding them lazily. So anything measured from a *viewport* edge
  (`last_vline`, and so any wrapped row count) is computed from the previous width's layout, and
  `Lines::last_vline` caches its answer, which turns one bad frame into a permanently wrong box that
  only a keystroke re-measures. Track **`screen_lines`** for that: floem `update`s it once
  `update_screen_lines` has walked the visual lines, and walking them is what rebuilds the layouts.
  `edit_field`'s auto-grow tracks both — the guard is `viewport.width() >= 1.0` (a zero-width
  measurement wraps at `MIN_WRAPPED_WIDTH` and inflates the box), the *count* comes after
  `screen_lines`.
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
  **A *style* closure is the same fact at a different price**, and it is the one a drag pays.
  `floem_reactive`'s `set` and `update` are both a bare `update_value` (`write.rs:34-51`) with no
  equality check anywhere, so a redundant write re-runs every style closure reading the signal as
  well as every `dyn_container` keyed on it. Both panel-width publishers in `lib.rs` are therefore
  guarded on `get_untracked() != w`, the shape `reveal_panel` already used: a divider drag is
  60–120 `PointerMove`s a second and each one re-runs the effect that publishes the *rendered*
  width, which `schema_tree::tree_row_min_w` reads from the style closure of every rendered tree
  row — so a drag held past `schema_min_w()`, where the clamp returns the same number every frame,
  restyled the whole tree for no visual change at all. It is also what keeps `schema_tree`'s reason
  for **not** memoising that per-row closure true: a restyle is a theme switch, a scale change or a
  panel resize, not a frame — and a resize *is* frames, so the guard buys the premise back instead
  of paying for a memo.
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
  **The two account forms are the same bug in its other flavour**, found later and in two more
  places: no fetch anywhere near them, just a key reading the draft signal that every keystroke
  writes, so typing a name rebuilt the form around the field being typed in. They take a memo over
  a pure *shape* function (`account_editor::{account_form_shape, grant_form_shape}`) rather than
  `overlay_open_key`, because what must not rebuild them is a value **inside** the form rather than
  a patch arriving from outside it.
  **And only the key closure is wrapped in an effect — the *builder* is called outside it**, so a
  scaled metric read there subscribes nothing and freezes at the scale the view was built at. Two
  sites paid for that. `schema_tree`'s `SchemaTreeCtx` therefore carries `indent_levels: u32`, a
  **count and not a length**: it was `level_indent()` in pixels, read in the `Loaded` builder, so on a
  live change to 160% every row's `padding_left` (`col_pad() + indent`) grew in its `col_pad()` term
  while the indent stayed at 16 instead of 26 — a schema group's children 10px left of where the same
  call site put their parent, per level, in a panel mounted for the life of the window and so never
  rebuilt. It self-healed on a collapse/expand, which is what made it read as a glitch. The count is
  multiplied by `level_indent()` *inside* each style closure, and a count cannot be frozen at a scale
  because it does not know about one. The grid's stored column widths are the same fact where the
  fix has to be an effect instead — see `rescale_widths` under *Data grid*.
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
  pad left by the free space — measure with a throwaway `TextLayout` at `font_body()` (same global
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
  overlays (completion popup, statement highlight, squiggles, Ctrl+K's question bar and verdict
  footer, run menu) each add back `EDITOR_PAD_TOP` to their `y`. **Ctrl+K's *diff* is not on that
  list and is not an overlay at all** — it is phantom rows inside the editor's own line flow
  (`inline_diff`), so it is laid out, scrolled and padded as text; only the bar above it and the
  footer below it are anchored, and the footer's `y` comes from `inline_footer_y`, which adds
  `EDITOR_PAD_TOP` like the rest. The built-in scrollbars float at the *content* edge (can only inset
  inward), so they're **replaced with custom overlay scrollbars** (`v_scrollbar`/`h_scrollbar` in
  `editor_area`): built-in bars hidden (zero-`Thickness` + transparent `Handle`), two `empty()`
  thumbs pinned to the border (`inset_right/bottom(3)`) with `autohide_state()`. Geometry from
  `ed.viewport` (offset `x0`/`y0` + visible `width()`/`height()`) vs. content (`ed.max_line_width()`,
  `(ed.last_vline()+1) * ed.line_height(0)`), in `v_geo`/`h_geo` shared by the style closure and drag
  handler. **The vertical content is taller than the text**: the editor sets `ScrollBeyondLastLine`,
  so Floem lays it out as `max(text, viewport)` plus a bottom margin of `min(viewport, text) − one
  line` — the virtual space that lets the last row be scrolled up to the top, which makes the maximum
  scroll `text − one line` whether or not the document overflows. `scrollbar_geo` (tested) adds that
  margin back through `consts::body_scroll_h`, the same function the results grid is sized by;
  measured against the text alone the thumb bottomed out a viewport early, and a document that
  merely *fits* showed no bar while the wheel still moved it. Thumb `.style()` reads `viewport.get()` **and**
  `query.get()` (content size isn't a signal). **Draggable**: `PointerDown` records grab offset +
  `id.request_active()` (pointer capture); each `PointerMove` sets `ed.scroll_to.set(Some(Vec2))`
  (it's `Option<Vec2>`, not `Point`). Thumbs use `scrollbar_hover()` + `CursorStyle::Default`.
- **Phantom text is how you put rows in the editor that are not in the document — and its list is
  order-sensitive.** A `Document`'s `phantom_text` hook returns text that is combined into a line's
  layout without ever entering the rope; it is the facility inlay hints use, and `inline_diff`
  renders the whole Ctrl+K suggestion through it. It may be **multiline** — a `\n` in a phantom lays
  out as an extra visual row, which is exactly why `TextLayoutLine::line_count` counts the line's
  nonempty layouts instead of assuming one per logical line, and why anything positioning by row has
  to go through that count. `PhantomTextLine.text` is walked **in stored order**, accumulating a
  column shift as it goes (`combine_with_text`), and floem never re-sorts it: the columns must be
  non-decreasing, so a block rendering *before* the line (column 0) has to lead the list and an
  end-of-line block has to trail it. Per-token colour therefore works by pushing one `PhantomText`
  per token — each carries a single `fg`, and the order you give them is the order they render in.
  **An added line that is empty has to render as a single space**: `relevant_layouts()` filters out
  layouts with no glyphs, so a genuinely empty row collapses and the block comes out one row shorter
  than it claims. And **writing the signal a phantom is built from changes nothing on screen by
  itself** — phantom text is baked into the line's cached `TextLayout` and that cache is keyed on
  `cache_rev`, so the write has to bump it or the rows appear a frame late, or not until the next
  keystroke happens to invalidate the line. `inline_diff::set_preview` is the one place that pair is
  spelled out, and the only way the preview should be set.
- **Wrap the editor's document *last*: `TextEditor`'s document-callback builders silently do
  nothing on a wrapped doc.** `update`, `placeholder` and `pre_command` each resolve the document
  with `downcast_rc::<TextDocument>()` and are written `if let Some(doc) = self.text_doc() { … }`
  (`floem-0.2.0/src/views/text_editor.rs:480,516,527`) — no error, no warning, no return value to
  check. `use_doc` installed *before* `.update(…)` therefore registered the `query.set(text)` sync
  on nothing, and the signal every consumer treats as the tab's SQL stopped following the editor
  entirely: autosave, Run, diagnostics and Ctrl+K's own statement lookup all read whatever text the
  tab happened to open with. It surfaced as "Ctrl+K always picks the first statement", which is
  what a stale `query` looks like once `statement_range` clamps an out-of-range offset — the
  wrapper was two months of features away from the symptom. So `use_doc` is the **last** builder in
  `editor_pane`'s chain, after `.update(…)`, and `SqlStyling` is built over the *inner* document on
  purpose (same rope, same `cache_rev` — the wrapper delegates both — and no dependency on a
  wrapper installed later). **There is no regression test**: it is a view-wiring failure in a
  builder chain that needs a running Floem app to observe, so the comment at the call site and this
  entry are the whole guard.
  The same family, one level down: **a `Document` wrapper inherits the trait default for everything
  it does not override, and the default is not always what the wrapped document said.**
  `InlineDiffDoc` took `has_multiline_phantom`'s default of `true`, where the `TextDocument` under it
  answers `false` for any non-empty buffer. Floem's `is_linear()` is
  `wrap == None && !has_multiline_phantom()` and word wrap is off by default here, so the wrapper
  quietly took the SQL editor off its linear visual-line mapping — every document, the whole session,
  for a feature that is live for a few seconds at a time. It now answers `true` only while a plan is
  published and delegates otherwise. Overriding one method is not the end of the audit: read the
  trait's other defaults and ask which of them the wrapped document was answering differently.
- **`ed.read_only` is a shared flag with two owners, and it does not gate `Document::edit`.**
  Anything that flips it must **save what it found and restore that**, never assume `false`, and
  must not treat it as a lock on the document. Floem checks it in `receive_char` and `run_command`
  only, so `doc.edit_single` — which goes through `Document::edit` — writes straight through it.
  Both halves of that were live bugs at once. The auto-pair handler flips it true for one key
  dispatch to suppress Floem's unconditional built-in char insert (the editor inserts the typed
  character after the handler returns, ignoring `CommandExecuted`), and its comment claimed to be the
  only reader; then the Ctrl+K preview began freezing the buffer with the same flag while a
  suggestion is on screen. A bracket typed during a diff therefore edited *through* the freeze, and
  the handler's deferred `set(false)` then cleared the freeze for the rest of the preview. The
  handler now returns early when the flag is already set and restores the value it found. A flag two
  features reach for is not a lock; if a third one arrives, this is the paragraph it has to read.
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
- **A pointer event stops at the first child that accepts pointer events — handled or not.**
  `EventCx`'s child loop (`floem-0.2.0/src/context.rs:143-158`) walks the children in reverse,
  dispatches to the first one `should_send` passes, and then does `if event.is_pointer() { break }`.
  For a pointer event (**the wheel included**) that break ends the search whether or not anything
  was processed, so an absolute overlay lying across another view kills scrolling over itself, and
  *not handling* the event does not help — there is no "pass it on" once your view is eligible.
  There are two ways out, and which one applies is decided by whether the overlay needs clicks.
  **Forward the wheel** if it does: take the event and push its delta into the target's own scroll
  channel (`ed.scroll_delta`), which is what Floem's editor gutter does with this exact problem
  (`floem-0.2.0/src/views/editor/view.rs:1112`) and what the inline-diff verdict bar does, so it can
  hold its Accept/Reject buttons and set a cursor while a wheel over it still scrolls the document.
  **The forward goes on every eligible view, not on the container** — the `break` happens at
  whichever *child* the pointer is over, so the bar's own handler covered the bar and nothing else:
  scrolling worked over the strip and died over the two words and the change count, which reads as
  intermittent swallowing rather than as a missing handler. All four carry it.
  **`pointer_events(false)`** otherwise: it takes the view out of `should_send` entirely, and is
  right for something with nothing to click — the inline-diff band strips, the statement highlight,
  the squiggles.
  **A container introduced for layout or arity reasons is a hit target too**, and this is the trap
  rather than any one overlay. `absolute().inset(0)` on a wrapper whose children are small and
  edge-pinned turns a few thin overlays into a single pane-sized one, and nothing about it says so:
  it paints nothing and looks like grouping. `stack`'s **16-child limit** is what tempts the edit —
  wrapping two views to free a slot is the obvious move — and doing it to the editor's two custom
  scrollbars put a full-pane, pointer-taking view above the editor, which by the rule above ate
  every click, drag and wheel in it. The editor was dead to input, from a change that was about
  arity. They are listed one by one now (exactly 16), with a comment at the site saying never to
  group them. The `stack((editor_box, inline_band_view))` nesting is safe by contrast because it
  wraps the editor *itself* rather than floating above it: the question is never how big the wrapper
  is, but whether it sits over other interactive views.
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
- **A deferred action's generation guard must live in the same scope as the state it clears, or in a
  longer-lived one.** The idiom above usually has a second half: the timer compares itself against a
  generation before acting (`if save_gen.try_get_untracked() == Some(g)`), which asks *did something
  newer happen?* and stands down on `Some(newer)` and on `None` alike. Put that guard in a
  shorter-lived scope than the signal it clears and the question inverts into a permanent refusal —
  every late timer reads `None` from a disposed generation and declines to do the one thing it was
  armed for. Manage Connections' `save_flash` was correctly hoisted into the modal's stable scope so
  that a late timer would never fire on a dead signal, but `save_gen` stayed inside `conn_form`,
  which the open/close `dyn_container` disposes: close the modal inside the 2s `SAVE_FLASH` and the
  check mark was still sitting on the Save button the next time it opened. Hoisting the flash out
  was necessary and not sufficient. **And a transient confirmation is better withdrawn when the
  surface showing it goes away** than left to its own timer, which is the fix taken there — a check
  reporting a save from a previous visit is wrong even while its two seconds are still running.
- **`.clip()` makes a flex item shrink-to-content — it won't stretch to its parent**, so a
  `flex_grow` spacer inside it collapses (right-aligned children stop reaching the edge). Put
  `.clip()` on a container with a *definite* size (the fixed-width `v_stack`), not the flex row you
  depend on for stretch. (Bit the find/replace bar's `All` alignment.)
  **`.clip()` is not a style — it is a node, so styling a clipped view means styling two of them.**
  `floem::views::clip` (`floem-0.2.0/src/views/clip.rs`) wraps the child in an unstyled `Clip`, so a
  `.style(…)` written *before* it lands on the child and the `Clip` sits between that child and the
  parent with `width: auto`, `min_width: auto`. That stays invisible until the clipped view wraps
  text. A `RichText` soft-wraps only when it is handed a width narrower than the line it holds, and
  both SQL previews collapse a body to one long line (`snippet::collapsed`, `history::preview`), so
  the wrapping is entirely at the mercy of the width the `Clip` resolves to. In a `flex_col` parent
  width is the *cross* axis, where no content-based minimum applies: the auto-width `Clip` stretches
  to the row and the text wrapped for free, which is why the snippet library and Query History
  previews wrapped without anyone asking them to. Wrap the same view in a `container` to give it a
  background and floem's `container` is a **row** (taffy's default direction, and `Container` sets
  none) — the `Clip` becomes a main-axis flex item, whose automatic minimum size is its content's,
  i.e. the whole statement. It took that width, the text's `width_full` resolved against it, and the
  library's three-line preview became one line cut off at the panel edge (fixed in `ecfe595`, after
  two attempts that put the same `min_width(0)` on the nodes either side of the `Clip` — the
  container above it and the text below — and changed nothing, because neither is the flex item the
  parent measures). The fix is `min_width(0)` on the `Clip` — a
  `.style(…)` applied *after* `.clip()` — and both panels now carry it on all three of the text, the
  `Clip` and the surface container. Adding a background container around a wrapping text view is
  what turns the omitted second style into a visible bug.
- **`tooltip()` re-parents its child, so every style chained *after* it lands on the wrapper and not
  on your view.** It is not a decorator: `floem::views::tooltip` (floem-0.2.0,
  `src/views/tooltip.rs:45-51`) mints its own `ViewId` and calls `id.set_children(vec![child])`, so
  the view you wrote is no longer the view the next `.style()` in the chain sees. That is the
  general hazard with **any constructor that wraps rather than decorates**, and the `.clip()` bullet
  above is the same fact read from the other end — but the two fail in *opposite* directions, so
  neither rule generalises from the other: with `.clip()` you must style **after** to reach the
  wrapper, with `.tooltip()` you must style **before** to reach your own view. The connection
  switcher's rows chained `menu_item_style` after `.tooltip(…)`, so the whole shared row style —
  `width_full()`, `flex_row()`, the gap, the horizontal padding — went on the tooltip wrapper while
  the `h_stack` underneath kept `width: auto` and sized to its own content. Its `flex_grow` spacer
  therefore never saw free space, collapsed to its 20px floor, and each connection's `host:port` sat
  a fixed 20px past its own name at a different offset per row instead of flush at the panel's edge.
  Two things hide it. The **hover background is in that same shared style**, so it sat on the
  full-width wrapper and every row highlighted edge to edge — the one cue that would have said "this
  row is narrow" was displaced along with everything else. And it presents as a *width* bug, which
  makes the panel's `min_width(400)` the natural suspect; the panel is innocent, and taffy
  right-aligns under a `min_width` perfectly well — the tree was rebuilt headlessly on taffy 0.4.4
  and the `min_width` variant lands the endpoints flush at the same right edge the explicit-`width`
  variant does. So do not go looking for a definite width here; the fix is the **order** — style the
  child, *then* wrap, then give the wrapper `width_full()` so the row has a width to resolve its own
  `width_full()` against. This is the only menu in the app whose rows carry a tooltip, which is why
  it is the only one that had it.
- **`s.hide()`/`s.flex()` (display none/flex) beat height/scale for a reactive show-hide** — adds/
  removes the element from layout cleanly (no clip/overflow/leftover space). Prefer it to animating
  height when you don't need the animation.
- **In-flow reveal animations are janky; only `.absolute()` transforms animate smoothly.** A
  `.transition(Height, …)` on an in-flow element reflows its container every frame and Floem only
  steps transitions on redraw ticks → ~5fps. Smooth animation here means animating an `.absolute()`
  overlay's inset/size so nothing reflows — which is what the Ctrl+K box did to grow from prompt to
  diff, until it stopped being a box (`inline_diff`) and there was nothing left to animate between.
  Either animate an absolute overlay or toggle `display`.
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
  listeners run.
  **"The root view" means the view the app's view function *returned*, and nothing inside it.**
  That distinction is the whole of a bug this survived a release with. `WindowHandle` keeps
  `main_view` = `view_fn(window_id).id()`, and the unconsumed-key fallback is
  `main_view.apply_event(listener, event)` — `ViewId::apply_event` reads the listeners registered on
  **that one id** and never walks children. The focus path is no help either: it dispatches
  *downward* from the focused view, so an ancestor is not a bubble target. `69fd7aa` wrapped
  `workspace`'s root in an outer stack to hold the eight window resize zones and left the KeyDown
  listener on the inner `root` — one level down, and therefore unreachable. Every branch in it went
  dead whenever focus was outside the SQL editor: Escape closing an open dropdown, the Tab-trap
  backstop, `NavKeys` (Ctrl+P, Ctrl+Shift+P, Ctrl+T/W, Ctrl+Tab, Ctrl+1-9, Ctrl+O/S) and the three
  panel toggles. **It looked fine because `editor_pane` answers the same keys in its own handler
  and the editor usually has focus** — the app's most-used surface masking the failure everywhere
  else. `lib::window_key_gate` now asserts the listener is attached after `chrome.resize_zones()`,
  i.e. onto the returned stack; it is crude on purpose (a precise check would have to parse the
  builder chain) and fails on exactly the mistake that was made.
  **Restoring its reachability restored the branches without restoring a guard**, so the handler
  now opens with `if modal_up() { return Continue }` — everything below it acts on the workspace
  *behind* the backdrop, and none of it should. The Escape and Tab branches sit above the guard on
  purpose: the first closes a control's popup *inside* a modal, the second steps the innermost
  modal's own focus ring, and both are modal-aware by design. A modal's focus root consumes nothing
  but Tab, so KeyDown reaches this handler whenever focus is on its root or on a button rather than
  in a text field — floem's editor is what swallowed these keys before, which is why the failure
  read as intermittent. Two were serious: Ctrl+W mid-confirm `set` the single-slot `Confirm` signal
  over `close_tabs_seq`'s parked continuation, so the chain's `resolve` was dropped and the
  remaining tabs were neither closed nor reported; and Ctrl+P mounted Find Anywhere — the modal
  layer's *bottom* entry, right for a palette raised before a modal and wrong for one raised while
  one is up — invisibly behind the modal, where its autofocusing field took the keyboard and Enter
  opened a row of a list nobody could see. The **position** is what the gate asserts, not the
  presence: after `innermost_ring_root()`, before `navkeys.handle`.
  Floem's editor consumes every `KeyDown`, so a focused `edit_field` used to swallow
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
  the fields below it on screen.
  **The number follows the layout, and the spacing is what keeps it able to.** Two ways it has gone
  wrong, both invisible to whoever added the control and both found by review rather than by use.
  A control *appended by number* rather than inserted at its place walks the user backwards: the
  settings modal's statement-timeout dropdown was written last and numbered 230 while sitting
  second in its group, so Tab ran row limit → confirm → validate → back up to timeout. And a block
  that **grows into the next one** collides: the event editor's schedule sub-form reached
  `TAB_SCHED + 30` = 60, which was also its `TAB_BODY`, and `FocusRing::register` inserts *after* an
  equal index rather than erroring — so the order fell out of which control happened to build
  first. A variable-length block therefore gets a decade of its own with nothing above it until the
  next hundred (`event_editor`'s `TAB_SCHED` 30–99, `TAB_OPT` 200–999), and
  `event_editor::tests::no_two_controls_claim_the_same_tab_stop` is the pin. A picker joins through
  `settings::in_ring_picker`, which is the whole apparatus below in one place; `focusable_dropdown`
  (settings' `Copy` picker) and `table_designer::focusable_owned_dropdown` (the designer/editors',
  since a table name isn't `Copy`) are both thin wrappers over it, and neither has an un-focusable
  sibling left — a second one would only be a way to leave a control out of the ring by accident.
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
  **And a deferred hand-back stands down for a claim taken after it** — `widgets::claim_keyboard`,
  quoted back through `keyboard_claim` / `keyboard_claim_unchanged`. Being deferred is what put
  `refocus_grid` in a race with the *other* immediate timer the same gesture schedules:
  `edit_field`'s autofocus. Pick **Optimize** (or any of the three AI fixes) off the editor's
  right-click menu and one update pass opens the Ctrl+K bar — queueing its prompt field's autofocus
  — and tears the menu panel down, whose `focus_root` cleanup calls `hand_keyboard_back`, which
  finds no other overlay and queues the workspace's home. Two timers due immediately, scheduled
  microseconds apart, and whichever lands last owns the keyboard: when that was the grid, the bar
  stood open with the keyboard behind it and Escape cleared a cell selection instead of closing it.
  It read as intermittent — about one opening in three — because it *was*. So an autofocus, and the
  Ctrl+K bar's own two hand-offs to the editor, **claim**; `refocus_grid` snapshots the claim when
  it is scheduled and refuses to fire if it has moved. A generation, not a "focus is held" flag,
  because floem's `AppState::focus` is private to it and a flag of our own would need clearing by
  every path that drops focus, floem's included; a counter answers the only question a deferred
  mover has — *is my hand-back still the latest word on the keyboard?* — and settles the race in
  both orders, which is the property the test pins.
  This is the durable form of what
  `set_menu_return` does for one case: fixing the sites one at a time is what produced the tree's
  cursor regression below.
- **A `.hide()`n control is still in the Tab order** — `hide()` is `display: none`, so the view is
  still in the tree and still registered in the ring, and Tab moves focus onto something nobody can
  see. Every engine-conditional block that was built-and-hidden is therefore now **built
  conditionally**: import's CSV and Excel settings, the designer's MySQL-only engine/collation and
  `ON UPDATE` and PostgreSQL-only index method/predicate, the view editor's MySQL options, PG
  recreate toggle and SQLite-only column list, the trigger form's `Fires`/`When` and SQLite-only
  `Of columns`.
  Nothing is lost by rebuilding — each of those binds
  straight to a draft or a persisted signal — and a control an engine can't express shouldn't be
  reachable at all, which is the same call `trigger_editor`'s per-engine form already made.
  **The else-arm is still `display:none`, via `widgets::nothing()`** — taffy skips a `display:none`
  child when it distributes `gap` but counts a zero-sized one, so a bare `empty()` arm leaves a
  whole `form_gap()` of dead space where the block would have been. The rule is about *controls*: an
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
  pinned to the scroll position the popup opened at. The signature hint's own two placement
  numbers are closures over `theme::scaled` for the same class of reason — it is lifted by its own
  height (two lines of text plus padding, all of which scale) and nudged right of the caret by air,
  and frozen at `48` the lift was correct at Normal only: from 130% the popup's bottom fell below
  the caret's line top and covered the statement being typed, which is the one thing a hint about
  that statement must not do. **`editor_area` also doesn't clip**, so an
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
  `completion_slack_w()` of air and rounds up, because a box sized to exactly its widest row puts
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
  still past it. `run_menu_w()` is one metric for the panel's `min_width` and the placement's
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
  separators as full rows shoved an upward-flipped panel tens of px too high. A row carrying a
  **detail** line (`MenuEntry::action_detail`, the faint second line the terminal picker's shells
  wear so `pwsh.exe` and `powershell.exe` are told apart) is the one row that is not one line tall,
  and both estimates know it: the height adds `MENU_DETAIL_LINE_H + MENU_DETAIL_GAP` and
  `menu_panel_width` takes the **wider** of the two lines, measuring the detail at `font_label()`
  where the label is measured at `font_title()`. Under-predicting is the dangerous direction — it
  says a panel fits below its anchor when it does not — which is why both are pinned at Normal and
  at Huge. **How that 30.5 is
  split matters away from 100%**, because `scaled()` is applied to each term separately: it was
  `MENU_LINE_H` 18.5 over a `MENU_ROW_PAD` of 6, while the estimate's own doc said 14 + 8 + 8 and
  `menu_row` *drew* a `font_title()` line inside `padding_vert(scaled(8.0))` — three decompositions
  of one row, all landing near 30.5 at Normal, which is why nothing caught it. The padding half is
  now the drawn one (`MENU_ROW_PAD` is 8.0 and `menu_row` reads that very constant, so the estimate
  and the row cannot state different numbers again) and `MENU_LINE_H` is 14.5, the residual of the
  *measured* total rather than a line-height theory that disagrees with it. Neither is measured
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
  then what the node can *show* you — `Properties`, `Live monitor`, `ER Diagram`, `Generate DDL` —
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
  **The near-collision the table is load-bearing for is `Refresh` (group 2) beside
  `Refresh view` (group 4)**, which a materialized view's menu carries both of: the first
  re-introspects the *database* into the tree, the second re-runs the view's query on the
  server and replaces every row it stores. Nothing but `group` keeps them on opposite sides
  of the separator, and a `Refresh view` sorted into the read group would put a write where
  the eye expects a reload.
  **The gate reads `overlays.rs` and nothing else** — `this_file()` hard-codes the path — and two
  menus in the app are outside it, both of which then shipped the inversion it was written to catch.
  The SCHEMA gear put `Create database` (group 4) *above* `Users and privileges` (group 2), and the
  tree's blank-space menu did the same; the gear now runs Refresh · Collapse all · Users and
  privileges · Create database · Show table sizes, and the blank-space menu carries its own half of
  the gate in `schema_tree::blank_space_is_a_subsequence_of_the_skeleton`, asserted over the entry
  **values** rather than over the source, since unlike the context menus those entries are data. It
  mirrors the group number for each of its labels from `group` here by hand, which is why
  `Users and privileges` is in this table at all: it is **not a context-menu row today**, but an
  unknown label fails, so a second menu cannot place an entry this one has never heard of. The
  vacuity guard (`arms.len() == 7`) counts arms of the builder the gate already reads, so it cannot
  notice a third source either.
  **`Export` is the one label the gate skips outright**, and it is an exemption rather than a third
  deviation: it has two honest homes — with `Generate DDL` on a database or a namespace, among the
  read entries that hand you what the node holds; directly below `Import` on a table, where the two
  are the file pair — so no single group can hold it without making one of the two placements a
  failure. It is safe to exempt because neither placement can be wrong in the way this gate exists to
  catch: it writes a file and never the server, so it is never the irreversible entry group 4's
  ordering keeps off the cursor, and the position that does matter is still pinned by
  `drop_is_the_last_entry_before_ai_explain`. The skip is a `continue` on that one label inside the
  test, not a lenient `group` — an unknown label still fails, which is the part that stops drift.
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
  **Which placement it asks for is part of the same answer.** `BelowIcon` tucks the panel under a
  ~28px glyph by opening 40px left of the icon's *right* edge; measured from a 220px enum field or
  a grid cell that is most of the way across the control, so a menu dropping from a **box** uses
  `BelowBox` instead — left edges flush, the same right-edge flip, and a *vertical* flip that clears
  the control instead of covering it (`box_menu_inset`; `BelowBox` carries the box's top edge for
  exactly that, `BelowIcon` doesn't and deliberately overlaps its glyph). The two are
  distinct variants rather than a flag because the anchor is also the menu's identity: a control
  compares the placement it *would* set against the open one, and two controls that disagreed about
  which variant they use would each fail to recognise the other's menu.
- **`popup_anchor` carries the menu's *identity*, not only its placement — so an opener must write
  it immediately before `popup_menu`.** One channel serves fifteen openers across ten files
  (`activity_panel`, `widgets::open_picker` — which is now every `<select>` in the app, not only the
  cell editors' — `connection_form`, `editor_pane`, `erd_view`,
  `grid` ×6, `lib`'s status bar, `monitor_view`, `table_designer::suggest_chevron`, `tabs`) and nothing in it says who filled it, so the grid
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
  eight `MenuId`s (`Popup`, `Context`, the five panel-owned flags — the schema eye and gear, the
  connection switcher, the active-database selector, the Server Activity clock — and `DatePick`, the
  date field's calendar, which is a *grid of days* rather than a menu but is dismissed by every
  gesture that dismisses one), gathered by `MenuFlags::of(&ui)` and closed by `close_except(keep)`.
  The calendar's membership is the whole reason the list is a list: it is opened from the row panel
  and from an open grid cell, it is closed by the workspace root, **and** it has to be closed by `GridState::dismiss_overlays`
  when a grid cell is clicked — a press the cell consumes, so the root never sees it. Reaching that
  through the shared list is what stopped it needing a second spelling in the grid. A trigger has to enforce this *itself*,
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
  each click-opened `MenuId` — `SchemaEye`, `SchemaGear`, `Connection`, `ActiveDb`, `ActivityClock`
  and `DatePick`, but not `Popup` or `Context`, which are opened on `SecondaryClick` where the root
  dismisses on the press and the opener runs on the release, one gesture — appears in a
  `close_except(Some(…))` somewhere, in any of the three spellings the crate uses for the path (`crate::widgets::MenuId::`,
  `widgets::MenuId::`, bare `MenuId::`). Deliberately
  weak: it counts `close_except(Some(` and `menu_trigger_press` registrations against the number of
  click-opened menus so a rename can't make it pass by finding nothing, but which site is which is
  not checkable from source. **`Popup` stays off the list even though several of its openers are
  click-opened** — the grid's toolbar dropdowns, `table_designer::suggest_chevron`, the cell
  editors' pickers — because that id names a *channel* many controls share, so one
  `close_except(Some(MenuId::Popup))` anywhere would satisfy a check meant to hold each of them.
  Each of those triggers still owes the call, and `widgets::open_picker` makes it for every picker; what is missing is a gate that can tell one from another. That list was written out three
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
- **An open context menu marks the row it acts on** — a 1px rule above and below in
  `theme::row_menu_edge`, painted by `widgets::row_menu_mark(s, marked)` from the row's own `.style()`.
  A menu is a panel floating clear of its row, and once the pointer is on the menu nothing on screen
  said which database, table or column the Drop was about. Two lists wear it. The **schema tree**
  (`Nav::menu_row`, `String` keys, through the one-line `schema_tree::menu_mark`) answered that
  first; **Manage Connections'** list (a local `menu_row: RwSignal<Option<u64>>`) needed it for the
  same reason and a sharper one — its menu's Duplicate and Delete carry the connection id
  themselves, and a right-click there deliberately does **not** select, because selecting runs
  `draft.load` and overwrites whatever is typed in the form. The helper therefore takes a plain
  `bool` rather than a row identity: that is what lets one definition serve two lists that identify a
  row differently, rather than the second one growing a lookalike that drifts. Neither mark is the
  selection or the nav cursor's highlight — a right-click moves neither (`resume_cursor` — a cursor
  that exists is the user's) — so this is a second, shorter-lived mark. The tree sets it in
  `marking_opener`, which wraps the row's `CtxOpener` where it is *built* rather than at the click,
  so the `Shift+F10` route marks too; the connections list has no keyboard menu route at all, so its
  `on_secondary_click_stop` is the only place that marks. Both clear through
  `widgets::clear_row_mark_on_close(menu, mark)`, an effect watching the menu channel go to `None`,
  which covers the closes a list's own code never sees — generic over both halves because the two
  keep the menu in **different channels** (the tree's `context_menu`, the connections list's
  `popup_menu`). Its guard, `mark.with_untracked(|k| k.is_some())` before the `set`, is not a
  micro-optimisation: `RwSignal::set` does not dedup, so an unguarded write re-notifies every row of
  a mark-less list on every close of any menu in the app.
  Two details are load-bearing. It is a **border, not an `outline`**: floem strokes a per-side border
  *inside* the view's rect (`paint_border`: top at y = 0.5, bottom at height − 0.5), so nothing bleeds
  onto the neighbouring rows and no `z_index` is needed to keep their hover backgrounds off it, while
  an `outline` — which floem inflates outward — would have needed one; and taffy sizes the **border
  box**, so `height(tree_row_h())` is unchanged and the rule costs 2px of content box, not a layout
  shift. That second half is why **the 2px is the caller's problem — only the caller knows where to
  take it from**. A connections row has no height at all: it is content plus
  `padding_vert(scaled(11.0))`, so a border grows it and shoves every list row below it down 2px for
  as long as the menu stands open, which is a list twitching under a right-click.
  `widgets::row_menu_mark_pad(base, marked)` returns one pixel less per side while marked, floored at
  0 — below 1px of base padding there is no pixel to give back, and a negative padding would take
  height *off* the row, the same shift inverted. The border is added in a style helper and the pixel
  is given back at the call site, so nothing but arithmetic says the two agree: `row_menu_mark_tests`
  pins them, and its load-bearing case is `the_rule_costs_a_padding_sized_row_no_height`, which
  asserts the *outer* box — padding ×2 plus border ×2 — is identical marked and unmarked, and was
  watched failing against the uncompensated version.
  The border-box half also holds for a row's **flex** children only, and the gap in it shipped a bug:
  taffy offsets a start-anchored *absolutely positioned* child from the border box plus the
  container's border on that side, so the 1px top rule moved `size_badge` — the tree's one absolute
  child — down by exactly 1px on right-click, and back when the menu closed, while the chevron, icon
  and name beside it never budged. The badge now centres on the cross axis
  (`align_self(AlignItems::Center)`), whose offset subtracts `content_box_inset` symmetrically and so
  cancels a rule that is on both edges; **any absolute child of a row that can wear this mark must
  centre rather than anchor to the edge the rule sits on**, and `size_badge` is the only one today.
  The key/index leaf is the only row outside the nav sequence, so it carries its own
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

## The results strip (and keeping a result)

**Every result is a `ResultPanel`, and the strip that lists them is on screen from the first run.**
A tab holds `result_tabs: RwSignal<Vec<ResultPanel>>` and `active_result: RwSignal<u64>` — a panel
**id**, never an index — and a tab with nothing run yet holds one `Idle` panel, so the strip is
never a bar with no chips in it. One run, one panel; a Run Everything batch, one per statement.
There is no longer a "single-grid path" beside a "batch path": `results_view` and the tab's
`results` signal are gone, and what replaced both is `results_multi`, which every result goes
through.
**Before the first run there is no bar at all** — `Tab::results_untouched` hides it with `s.hide()`,
because a strip holding one chip that says nothing, over a pane that says "Run a query", is two
pieces of furniture for one empty state and 28px off the grid for the rest of the session. It
appears with the run rather than with its rows (a `Running` panel is already not untouched) and goes
again only where the pane is empty for the same reason: every result closed, or the tab respawned.
The affordance it carries is still discoverable, because by the time there is anything to pin the
strip is up.

**Pinning is what the strip is for.** `resultsel::after_run` is the rule — a run replaces the
unpinned panels and leaves the pinned ones alone — so a result can be kept across later runs and
compared against them without a second tab and a second execution. That matters most where it is
least recoverable: re-running is the only way to get a result back, and against a table that is
changing, the "before" is simply gone. Right-click a chip for Pin/Unpin, Close, Close other results,
Close all results; the pinned block sits at the front in pin order, a pinned chip shows a pin where
its × would be, and "Close all" spares the pins — the query strip's rules, restated in
`core::resultsel` and applied by `Tab::begin_run` / `set_pinned` / `close_panels`.

- **A pinned result is frozen, and this is a safety rule rather than a nicety.** `ResultPanel::frozen`
  is asked by exactly two things, and everything else follows from them. It empties the **edit
  model** (the same way a read-only connection does), which stops cell editing, row insert/delete,
  the commit *and* server-side filter/sort in one move, since all four are gated on that model. And
  it makes `GridState::current_statement` return `None`, which withdraws the two affordances that
  re-run without going near the edit model — the capped notice's "read all rows" and the export
  menu's "All rows". The write half is the one that would have cost data: a grid write's `WHERE` is
  the key columns and their **original** values (`model::RowEdit`) with no guard on the columns being
  set, so a commit from a twenty-minute-old snapshot is a well-formed `UPDATE` that overwrites
  whatever landed in between — and the 1-row safety net cannot see it, because exactly one row *is*
  matched. The re-read half would have destroyed the snapshot itself. `run_query_core` asks
  `Tab::shown_frozen` once more before a view re-run, at the funnel, so a stale affordance can't get
  round it. The grid says so where the user is looking when they try to type: a `· kept — read-only`
  note on the toolbar line, because a cell that silently refuses to open reads as a broken grid.
  **The answer is a `Memo` over one panel (`Tab::panel_frozen_memo(id)`)**, and both halves of that
  sentence are a bug that shipped.
  *Not a sampled `bool`*: pinning the result on screen moves neither its id nor its phase, so it does
  **not** rebuild the grid — a flag read in the grid's builder went on saying "not pinned" for as
  long as that grid stayed mounted, leaving the just-pinned result editable, with its row actions and
  its filter bar, until the user switched away and back. Tracked, the edit-model effect recomputes on
  the pin itself, exactly as it already did for `read_only` beside it, and everything gated on that
  model follows.
  *And over **one panel**, not over "whichever panel is shown"*: asked the second way it **crashed the
  app**, on the plainest gesture the feature has — run a query with a pin present. The shown panel
  changes and the grid is unmounted in the same update pass, so a flip re-ran that grid's edit-model
  effect on its way out, which rebuilt the toolbar's `ai_menu` (a `dyn_container` keyed on the model),
  whose new child's style effect read a `GridState` signal whose scope had just been disposed —
  `Option::unwrap()` on a `None` in `floem_reactive::read`. **A view must only subscribe to facts
  about itself**, or a change that unmounts it races the change it is listening for; and a panel that
  has *gone* keeps its last answer rather than reporting `false` on its way out, so closing the shown
  result cannot flip it at teardown either. The `· kept — read-only` note is shown and hidden by
  `s.hide()` for the same reason — the *Floem 0.2 gotchas* rule, and here also the safe construction.
  `Tab::shown_frozen` is the untracked half of the pair, for callers acting *now* inside an event
  handler; anything answering the question while a grid is mounted takes the memo.
- **What a panel remembers.** `PanelView` — column widths (with the `grid_char_w` they were measured
  against), the client-side sort, and the frozen column — as **signals in the panel's own child
  scope**, not fields in the panel list: the grid writes a width on every mouse-move of a resize
  drag, and through the list that would clone and re-notify the whole strip each time. `GridState::new`
  seeds from them (length-checked against the column count, so a restore can't leave the header and
  the body disagreeing) and four effects in `grid_view` mirror changes back. Selection is
  deliberately *not* remembered: it is where the user last clicked, not a property of the result.
  Nothing is persisted — a strip is session-only, like the results themselves.
  The frozen column is `frozen_col`, spelled out because a *frozen column* and a *frozen result* are
  different things one field apart (`gctx.panel.frozen_col` is an index, `gctx.panel_frozen` is
  whether the result is pinned), and two `.frozen`s in one data path is a wrong-variable bug waiting
  for its reader.
- **The list and the selection move together, under `batch`.** `begin_run`, `close_panels` and
  `set_pinned` each rewrite `result_tabs` *and* `active_result`; separately, the moment between them
  has `active_result` naming a panel the list no longer holds, `shown_panel` falls through to its
  first-panel fallback, and the body builds that panel's grid in full — `init_widths` sampling 200
  rows across every column, `analyze_edit`, `column_editors` — only to throw it away when the
  selection lands. With a pinned result present that was every run. `floem_reactive::batch` holds
  the effects until both writes are in.
- **A bar describes the result it ran on, so switching results clears it.** `view_err`, `commit_err`
  and `commit_note` are tab-level and are cleared by the grid actions that supersede them (a new
  commit, a fresh filter re-run, a click on the filter bar) — none of which switching results is, so
  a bad `WHERE` typed on the live result kept its red bar over the pinned snapshot the user then
  clicked, and a failed commit followed them onto a result that cannot be committed at all. An
  effect beside the one that closes the find/goto/selection bars clears the three on a change of
  shown panel. `commit_wait` is deliberately exempt: it stands for a write still in flight and
  carries the one-click Rollback, so it belongs to the connection rather than to whatever result is
  on top of it.
- **The body's key is `(panel id, phase, load_gen)`, a deduped `Memo`.** The id makes switching
  results a remount (two panels can both be `Loaded` and be different results); the phase is what
  keeps an in-place commit splice from being one — it replaces the panel's `Arc` without touching
  the id or the nonce, so the grid, its scroll and its selection survive, which is the whole point of
  the splice. `sync_canonical` is pointed at *that panel*, so a splice lands on the result it came
  from. A run passes through `Running` and a filter re-run bumps `load_gen`; both rebuild, which is
  what they mean.
- **Ids, not indices, all the way through.** Panels are pinned (which reorders), closed (which
  renumbers) and replaced, so `shown_panel` finds by id with a first-panel fallback, the strip's
  `dyn_stack` is keyed on the id, and a batch writes each statement's result back by id — a
  positional write would land a statement's result on somebody else's panel when a pin reordered the
  strip mid-run. A result whose panel was **closed** while it ran lands nowhere, which is
  deliberately the same answer the run-generation check gives a superseded run.
- **What a chip says.** The statement, previewed (`history::preview`), capped at
  `result_title_avail()` — the chip's width less the 10px inset, the label's margin and the trailing
  glyph. **The cap is on the label, not on the chip**: `max_width` on the row only clips, so the
  text laid itself out at its natural width and ran under the × and out over the next chip, which is
  the arithmetic `tabs::tab_title_avail` pays for the query strip and for the same reason.
  **The chip's hitbox is the view that carries both the style and the listeners**, which is why the
  tooltip goes on the *label* and the inset is `padding_left` on the chip rather than a margin on
  the label: chained after `.tooltip()`, the chip's style painted a wrapper while the handlers sat
  on a text-sized box inside it, and every click outside the words was swallowed (see
  **`.tooltip()` wraps the view** under *Floem 0.2 gotchas*). The query strip is built the same way,
  which is why its tabs never had it. The tooltip carries the full text, the run's
  age (`history::relative_time`) and — for a loaded one — `ResultSet::retained_bytes`.
  The last is what makes "keep this result" an informed choice: a pin holds its columns' arenas and
  one 4-byte offset word per cell, which at the 200,000-row default cap is 40 MB across 50 columns
  before a single character of text. Cheap to *hold* (the panel keeps an `Arc`, so nothing is
  copied), not free to keep.

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
  The resulting **draw order** — frozen first, then the rest in index order — is
  `core::edit::visual_cols`, and it is what paste, Ctrl+C and an AI attachment walk (see `core::edit`)
  so they agree with the screen rather than with the index.
- **⚠️ Scroll-sync rule (cost a hang):** a scroll view must **never both read and write the same
  offset signal** — it re-enters its own layout and hangs the UI thread. Strict one-writer/one-reader:
  the **data pane writes `vscroll`** (`on_scroll`) and reads `gs.scroll_to` (keyboard channel); the
  **frozen pane reads `vscroll`** (its `scroll_to`), has **no `on_scroll`**, and never scrolls itself
  on the wheel — it consumes the event (`Stop`) and forwards the delta into `gs.scroll_to`, so the
  gutter isn't a dead zone under the pointer while the data pane still moves. The header pane
  forwards **both** axes the same way.
- **Virtual space under the last row.** Both bodies lay out to `consts::body_scroll_h` — the rows
  plus `min(viewport, rows) − one row` — so the last row can be scrolled up to the top rather than
  sitting on the bottom edge, and the maximum scroll is `rows − one row` whether or not the result
  overflows. It **overrides** the height `virtual_stack` gives itself (`rows × row_h`); a
  `margin_bottom` would not work, because `Scroll::child_size` reads the child's layout size and
  margin falls outside it. Held in a `Memo` beside `win` (recomputes on scroll, notifies only on a
  resize) that **both** panes read: a frozen pane sized to the bare rows would clamp its own
  `scroll_to` a viewport early and drift out of line exactly when the virtual space came into view.
  The two wheel handlers that clamp a vertical offset themselves (header, frozen) read that same
  memo — clamping either to `rows × row_h` stops the wheel early over that pane alone. The SQL
  editor gets the identical rule from Floem's `ScrollBeyondLastLine`, which is why `body_scroll_h`
  is one function and not two.
- **Column widths** (`gs.widths`) are estimated from content on load; the header's `col_resize_handle`
  drags to resize (moving-view trick) and double-clicks to auto-fit. Cells read `gs.widths` in
  `.style()` so resize is live. Every cell/header uses `flex_shrink(0)` so the row overflows (enabling
  h-scroll) instead of squeezing. **`cell_chrome_w()` is the one composition of what a cell spends on
  itself** beyond the value — `2 · grid_pad_h() + GRID_CELL_DIVIDER`, the divider counting because it
  is a *border* and so comes out of the content box too. Both estimators reserved a flat `22.0`,
  which is a pixel generous at Normal and short by 6px at 130% and 11px at 160%: **auto-fit clipped
  the value it exists to fit**. `numeric_edit_pad_left` composes the same three terms correctly,
  which is what makes this a composition bug rather than a wrong constant.
  **And stored widths are carried across a live scale change** (`rescale_widths` + `GridState::widths_at`,
  which holds the `grid_char_w()` the current widths were measured at). They cannot follow the scale
  the way everything else in the grid does, because they are pixels *stored* rather than read in a
  style closure — floem wraps only a `dyn_container`'s key closure in an effect and calls the builder
  outside it, so `init_widths`' `grid_char_w()` read subscribes nothing. Raising the scale with a
  result open grew every cell's font, padding and row height while the columns stayed cut for the old
  one, ellipsized across the board until the statement was re-run or every divider double-clicked;
  lowering it left every column ~1.6× wider than its content. It is an **effect** and deliberately
  not a scale term in the rebuild key: keying the grid would discard the child scope and take the
  selection, the scroll position and every staged edit with it, where this recomputes one signal.
  Every column is carried, **including one the user dragged** — a width chosen to fit that column's
  content should still fit it when the content is 1.6× bigger, so there is no `dragged` set to
  maintain — and `min_col_w()` is applied *after* the ratio, since the floor scales too (48px at
  Normal is under the 77px floor at Huge). A `ratio` that is not positive and finite leaves the
  widths alone: the old measurement beats a column collapsed to the floor.
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
  a batch of edits the user can still look at and throw away — staged through
  `stage_many`/`stage_new_many`, which are where the revert-to-original and
  blank-cell rules are applied (`stage`/`stage_new` are one-element calls into
  them) and which take **one** signal update for the whole paste: `dirty` and `new_rows` are read
  by the painter and by every derived view, so a per-cell update would invalidate the grid ten
  thousand times for one gesture. **What a blank means in a pending row is the caller's choice
  rather than the function's** (`core::edit::BlankCell` + `pending_cell`): a *typed* clear is
  `UnsetsIt` — clearing a field you typed into is an undo, so the column goes back to unset and the
  `INSERT` omits it, letting the server's default fill it — while a **pasted** blank is
  `IsAValue`, the empty string, because a paste is an assertion about values. That arm did not
  exist: `stage_new_many` applied the typed reading to everything, three lines from a `stage_many`
  that staged `''`, so one block wrote `''` above the pending-row boundary and the *server default*
  below it — the same clipboard cell, two stored values, decided by a line the user cannot see — and
  it discarded a value typed into a pending row whenever the cell pasted over it happened to be
  blank. The parse is
  `core::edit::parse_tsv_block`, **the exact inverse of `GridCells::tsv`**: split on newlines and
  tabs, no quote interpretation. A CSV-style reader here would be the obvious mistake — the copy
  side emits no quoting, so there is none to undo, and unquoting would silently turn a cell whose
  value genuinely is `"hello"` into `hello`. The cost is that a spreadsheet cell containing a
  newline arrives as two rows, which is the rarer wrong answer and a visible one. `plan_paste`
  lays the block over the grid: **one copied cell fills the whole selection** (that is how a column
  gets set to a constant), anything larger keeps **its own** shape from the selection's top-left,
  and everything is clipped to the display rows — pending new rows included, so a paste can fill
  rows the user just added. **It takes `frozen`, because that is what "extends" means**: a block
  grows into the columns drawn beside the anchor, which under a freeze are not the ones indexed
  beside it (`core::edit::visual_cols`), and the index walk it replaced put the second value of a
  two-wide paste on `email` into a frozen `ssn` at the far left of the screen. The single-value case
  still fills the *selection*, which is what is painted highlighted, so it needs no translation. What falls outside, what lands on a read-only column, and what lands
  on a row marked for deletion are **counted and reported** in the same bottom bar a commit error
  uses (set *after* staging, since `stage` clears it), because a paste that discarded half a
  spreadsheet looks exactly like one that worked. **Which surface** is
  `core::edit::paste_report`'s to decide, pushed down there so it can be tested: a partial paste is
  a `Notice` — an ordinary success with a caveat, on the ordinary chrome — while a paste that
  landed *nothing* is a `Failed` and earns the red fill. Both were errors, so "Pasted 5 cells,
  skipping 1 in read-only columns." was rendered indistinguishably from a write-back that failed.
  The counts are snapshotted through `PastePlan::counts` **before** staging drains the cell list,
  which is what stops the report claiming every paste landed nothing. **What landed is counted by
  the caller, after staging**, and handed over as `paste_report`'s `staged`: it used to be derived as
  `planned - skipped_deleted`, which is what the plan *intended*, and staging drops entries the plan
  cannot know about — `stage_many` un-stages a cell pasted back over its own original value (the
  rule itself is `core::edit::staged_cell`, in core with the NULL normalisation that makes it right;
  `stage_many` is a loop over it), and
  `stage_new_many` removes a column whose pasted cell is blank. So pasting a column's own values
  over itself reported `Pasted N cells` while `dirty` gained nothing at all. The two `stage_*_many`
  calls return what they changed and the caller sums them; `PasteCounts::planned` is gone, so no
  reader can rebuild the figure that was wrong. A read-only column is
  skipped **in place**, never
  shifted, which would write one column's values into the next. **One thing is interpreted, and
  exactly one**: the literal `NULL` resolves to SQL NULL (`edit::pasted_value`, applied inside
  `plan_paste`, so `PastePlan::cells` carries `Option<String>` and a value reaches the grid already
  an `Option`). It is what the copy side writes for a NULL cell — stored or staged — so reading it
  as text made a copied nullable column come back with every NULL in it replaced by the *string*
  `NULL`, invisible until the Commit after which `WHERE x IS NULL` no longer matched the row. The
  resolution sits in the plan rather than at the two staging calls because those are the seam
  `BlankCell` exists for: one of the pair got a rule and the other did not, and both compiled. The
  match is **exact** — `null`, `Null`, `NULL ` are text — and *typing* `NULL` into a cell still
  stages the string on every path, which is the escape hatch the ruling leaves and the reason a
  column legitimately holding the word `NULL` can still be written, just not pasted. The two writes
  therefore remain reachable from one keyboard, and `grid::cell_ink` → `CellInk` is what keeps them
  apart on screen: it replaced the painter's `if` chain, and a staged `None` renders in the same
  italic a NULL *original* has always had (white on the same green fill), where the two used to
  share one arm and `middle_name = NULL` was pixel-identical to `middle_name = 'NULL'` right up to
  the write. The italic is deliberately not a new vocabulary — "there is no value here" reads the
  same whether the emptiness is stored or staged. The **copy** side is unchanged: the clipboard
  carries the display text, so a spreadsheet still receives four readable characters. An open
  inline editor takes Ctrl+V back — `paste_selection` returns early while `edit_cell` is set,
  explicitly rather than trusting the text field to swallow the key first, because being wrong
  about the dispatch order costs a block overwrite instead of a caret insertion.
- **What a cell *says* is resolved in one place, and it isn't the view.** `copy_selection` and
  `attached_rows` read the signals once into `grid_cells` — a `core::edit::GridCells` borrow over
  `rs`, `order`, `formats`, `dirty` and `new_rows` — and ask it for `tsv(rect, frozen)` or
  `attached(rect, cap, frozen)`, both of which emit the selected columns in **draw** order
  (`visual_cols`) because the receiver reads them left to right; they contain no resolution of their
  own, and `displayed_cell_text` /
  `pending_cell_text` are gone. The reason is under `core::edit`: the rule went out one source
  short twice in the view, most recently without `format::apply`, so a `Timestamp` column attached
  the epoch integer the cell does not show. The **painter** is the exception and stays one:
  `data_cell`'s content `dyn_container` runs per cell per frame reading the signals one at a time,
  so it is the reference implementation `GridCells::text` is written against — the two must be
  changed together, and neither `grid_cells` nor its callers may become a per-frame path. Ctrl+C
  passes `formatted = false` and an attachment passes `true`, which is the *Copy formatted* entry's
  reason for existing. **How a cell is *weighted* is a decision too**, and it is `grid::cell_ink` →
  `CellInk` (`Staged`, `StagedNull`, `Absent`, `Fk`, `Plain`) rather than an `if` chain inside the
  style closure — the fifth case is why: a staged SQL NULL and a staged four-character `NULL` went
  through one arm and painted identically. A paste no longer produces the second by accident, but a
  *typed* `NULL` still does, so the distinction stays load-bearing (see the paste rules above).
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
  bundled in `GridCtx`, threaded `results_section → results_multi → loaded_view → grid_view`,
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
- **A column whose legal values are written down doesn't edit as text.** Booleans, `ENUM`s, `SET`s
  and dates get their own control, in the grid *and* in the row panel, over
  `core::celledit` + `core::date` (the rules) and `ui::cell_editors` (the widgets) — both of which
  say why each shape is what it is. What the grid adds is the plumbing:
  - **`gs.editors`** holds one `CellEditor` per result column, resolved by `column_editors` from
    the column's **declared** type — its base column in the loaded schema, falling back to the wire
    type. It is a signal filled by its own effect, with **tracked** reads of `db_nodes` and each
    node's `schema`, so a database whose introspection lands after the grid was built upgrades a
    text field into a dropdown in place. (`edit_model`'s effect deliberately does not track those;
    the two answer different questions and editability is settled at build.)
  - **The column's control is narrowed to the value in hand** before it is used, by `fitting_editor`
    (row panel) and the same `celledit::fits` call in `data_cell`'s editing branch. This is the seam
    the whole feature's safety rests on: the classification is per *column*, and one cell of that
    column may hold something the control cannot represent.
  - **In a cell**: booleans, enums and sets all edit through one control
    (`cell_pick_editor` — the value and a chevron, with the shared popup menu over it), opened
    against the cell one tick after the face is built because a view has no `layout_rect` until it
    has been laid out. A cell has room for a value and a chevron and nothing else, and a menu is the
    only list that can be drawn over a grid at all. Choosing commits immediately through
    `keep_cell_edit` — Enter's contract, including the pending-row hop — so a `SET` toggles one
    member per opening. The editor closes on **any** dismissal of that menu, which is why it watches
    the `popup` flag rather than only its own actions: the root's click-away handler writes that
    channel directly. **The cell drops its own horizontal padding** while such a control is open
    (`cell_fills(&open_cell_shape(..))`, asked by both the content and the style so the two cannot
    disagree): the control carries its own surface, and a padded cell around it reads as a box
    inside a box. What an open cell *is* — a plain field, a picker in place of one, or a field with
    the calendar beside it — is the pure `cell_shape(editor, buf)`, three shapes rather than "a
    control or not" because the two controls want opposite things from the cell around them.
    **And the menu may not outlive the cell.** The popup channel is window-global and nothing but a
    pointer-down anywhere clears it — not a tab switch, not a re-run, not a scrolled-away row — while
    the entries are `Rc` closures over this cell's signals, so clicking one after the scope is gone is
    a `get_untracked` on a disposed signal, i.e. `try_get_untracked().unwrap()`: a panic that takes
    the window and every other tab's uncommitted edits with it. `cell_pick_editor` and
    `cell_editors::pick_field` therefore carry the `on_cleanup` → `cell_editors::close_picker` that
    `cell_calendar_editor` already had, and `close_picker` takes down only a menu standing at **its
    own** anchor. That anchor is *remembered*, not recomputed: `open_picker` returns the anchor it
    left standing and each control holds it in a plain `Cell`, because a view's `layout_rect` is not
    something to ask about while its scope is being disposed and a signal read inside `on_cleanup` is
    the very hazard the cleanup exists to prevent. `keep_cell_edit` is the belt to that brace, and
    opens with the `gs.alive()` guard `drop_cell_edit` already had.
  - **In the row panel**: `typed_editor` puts the control inside the same NULL toggle a text field
    gets (`nullable_field`, extracted from `scalar_editor` for exactly this). Only the `SET`'s
    wrapping chips can outgrow a line, so only that row grows.
    **Every one of those controls owes the keyboard what the field it replaced gave it** — the
    panel's `autofocus` and its panel-closing Escape — and two of them ignored both, which is a
    column that cannot be set without a mouse: nothing took the keyboard when the panel opened on
    an `ENUM`, and Tab walked past the control as though it were a label. They are
    `keyboard_navigable` now, which is also their activation: floem fires `Click` on the focused
    view for Enter and Space, so the pointer's handler is the keyboard's handler, and a `SET`'s
    chips are a stop each because a row of independent toggles is what a checkbox group is. The
    contract is gated by a signature scan (`row_panel_focus_gate`) rather than trusted, since what
    went wrong was two parameters that were never passed.
    **The layer above it has its own gate, because a wrapper can withhold the keyboard before any
    control exists.** `nullable_field` and `json_field` each have a branch that builds no control at
    all — a nullable column that is NULL in this row renders the `<null>` sentinel and a **Set
    value** button — and both took `autofocus` only for the other branch, so a panel opened on a row
    whose first editable column was NULL took no keyboard: the arrows went on moving the grid's
    selection under an open panel, and reaching the first field cost a Tab walk or a click. All
    three wrappers (`field_mini_btn` included) now take it through, and that button is
    `widgets::key_pressable` + `cell_editors::focus_on_mount` — the first because Tab walking past a
    button as though it were a label is the other half of the same complaint, the second because
    focusable-on-mount and reachable-by-Tab are different properties. `grid::row_panel_null_gate`
    pins both, a source scan in the family with `popup_anchor_gate` and `menu_trigger_gate` and for
    the same reason: what went wrong was not a calculation but a parameter that never arrived.
    `key_pressable` is `in_ring_button`'s no-`FocusRing` sibling — the row panel and the activity
    panel join floem's own traversal, so the difference is only `keyboard_navigable` in place of
    `in_focus_ring` — and Enter and Space press it while **every other key carries on**, so Tab still
    walks past and the panel's Escape still reaches the panel. `focus_on_mount` is `pub(crate)` for
    the same sharing.
  - **Dates keep the text input** in both places, with the calendar beside it: typing a date is
    often faster, a `TIMESTAMP`'s time of day has no calendar to come from, and a value no picker
    can represent still has to be editable (`0000-00-00` gets a plain field and no panel at all).
    In a cell the panel is dropped from the cell's own rect, one tick late for `cell_pick_editor`'s
    reason, and `cell_calendar_editor` hands it the editor's **lifetime** rather than sharing it:
    - **The keyboard cannot decide when the editor closes while the panel is up.** Floem takes the
      window focus on *every* pointer-down and hands it back only to a focusable view under the
      cursor — a day, a month arrow and the Now button are none — so the first click inside the
      calendar reached the field's `FocusLost` as "the user left", closed the editor, and the pick
      it was about to make landed on a cell that was no longer being edited. `data_cell`'s
      `FocusLost` therefore stands down for a press the panel reports having taken, and the panel
      closes the editor instead: a chosen day stages through `keep_cell_edit` (the in-cell picker's
      commit, a cell having no Save button in reach) and every other way the panel goes away closes
      the editor without staging, which is the effect on the channel.
    - **The press is the question, not the panel.** `cell_editors::take_calendar_press` is a
      one-shot flag set by the panel's own pointer-down swallow and *taken* by the first asker,
      because floem clears the focus, dispatches the press, and only then emits the `FocusLost` it
      caused — so a flag set during dispatch is readable by the handler that follows it. Standing
      down for the whole time a panel was merely *open* was the first spelling, and it ate Escape:
      `text_input` answers Escape by dropping the window focus and reporting the key handled, so
      that key reaches this `FocusLost` and nothing else in the app, and swallowing it closed
      neither the editor nor the panel and left the grid with no keyboard. The field also drops a
      standing flag when it regains the caret, the one moment it is certainly stale.
    - **Standing down is only half of it: the guard hands the caret back.** Nothing else will —
      floem returns the focus only to a focusable view under the cursor, and a day is not one — so
      a field that merely survived the press was mounted and deaf, with a `DATETIME`'s time of day
      untypable and Enter/Tab going to the window root. The caret lands at the end of the value,
      `text_input` dropping its selection and moving the cursor there on regaining focus.
    - The field keeps working throughout — it still holds the value, Enter still stages it, Tab
      still hops — and its teardown clears the channel, because `edit_buf` belongs to the *next*
      cell to be edited as much as to this one.
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
  That means the **shown** result (`shown_panel_loaded`), not "any result in the tab": only one grid
  is mounted, so switching from a loaded Result 1 to a failed Result 2 was the same bug one level
  along.
- **A failed statement reports in the editor's error bar, and only there.** The pane keeps a dim
  "Statement failed." — a server error is one long line, and as the pane it rendered unwrapped
  across the middle of the window and out over the schema sidebar. The message goes under the SQL
  that produced it, beside the **Explain** and **AI fix** that act on it (`intel::error_fix_range`
  scopes the fix to the failing statement, so this works for one statement of a script as well as
  for a whole run). It used to be split: a single run's went to the editor bar and a batch
  statement's to the panel bar (`batch_err`), because `run_all` cleared the tab's `results` and left
  a batch with no editor bar to fall back on. Once **every** result became a panel the two were the
  same value, and the pair drew the same error twice — so `batch_err` is gone and `grid_error_bar`
  now reports only on what the *grid* did: a commit, a filter re-run, an export. None of those is a
  statement in the buffer, which is why nothing it carries is `fixable` any more.
  The editor bar keys on a `Memo<Option<String>>` of the message rather than on the `QueryState`
  itself: the shown result is derived from the panel list, so every write to it — each statement of
  a batch landing, a pin, a filter re-run restating its panel — reaches that container, and
  `QueryState` has no `PartialEq` to dedup on. Keyed on the state it rebuilt the bar on all of them,
  including on every keystroke, since typing clears a stale error through the same signal.
- **That bar has four states on two surfaces, and the surface is the message.** The red fill means
  an error; the ordinary chrome carries the wait note (a write taking long enough to explain, with
  its one-click `Rollback`), a plain **note** — something worth saying about an operation that
  *worked* — and the running export above. The note exists because there was no non-red channel: a
  partial paste used `commit_err`, so an ordinary success was drawn in the colour that means a
  write-back failed. The six signals travel as one `BarSignals`, whose `any_up` is asked by the
  bar's own style **and** by the selection summary that lifts itself above it — two hand-written
  copies of the same `is_some` chain before, which a new surface would have had to be added to
  twice, and a bar that is up while the summary thinks it isn't is the two of them drawn on top of
  each other. **What travels in `BarSignals` is state, never an action**: `export_cancel` is an
  `Rc<dyn Fn()>` parameter of `grid_error_bar` beside `rollback_tx`, because the struct is
  `#[derive(Clone, Copy)]` and an `Rc` in it would cost every reader of `any_up` a clone.
  `results_section` reads the flag and the action off `GridCtx`, so `GridState` keeps no copy of the
  latter. On the writing side, `GridState::clear_bar` is the one way the bar comes down: seven
  identical copies of `if commit_err.is_some() { … }` were what a second signal would otherwise have
  had to be added to seven times. **`discard_edits` takes the whole bar down, not just its error
  surface** — Discard means "none of that is true any more", and the *note* surface is the one that
  can hold a sentence about the edits just thrown away, so `Pasted 5 cells, skipping 1 in read-only
  columns.` stood over five cells that no longer existed until some other path happened to clear it.
  That site read `clear_if_any(gs.commit_err)` rather than the `is_some()` shape, which is why the
  sweep that found the other two copies of this missed it.
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
  `1,000 of ~4.2m rows` rather than `1,000 rows (capped)`. Three things have to line up, and
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
  one. A repeated ask is free: the slot is the guard. The wording of the segment — including
  whether `(capped)` is still worth saying once the comparison is there, and which figure the noun
  agrees with — belongs to `stats::rows_read_clause`, not to the view; the view supplies
  `truncated` and the total and prints what comes back. The cap itself is unchanged and is still a
  **client-side stream cutoff**, not a `LIMIT`; this only says how much of the table went past it.
- **The Download menu names the scope, and only when there is a choice to make.** `export_menu`
  normally opens the flat six formats — all of them, unlike the **Copy** menu beside it, which
  filters on `export::ExportFormat::is_text` and so has no Excel entry: a file can hold bytes and a
  clipboard render cannot (the empty-string trap is in `core::export`'s entry). When the result is
  `truncated` **and** `GridState::current_statement` yields something to re-run, it puts a scope
  step in front of them —
  `Fetched rows (N)` and `All rows (~N)`, both through `text::human_count`, the `~` for the same
  reason the stats line carries one (the figure is the catalogue's estimate, and a menu promising an
  exact count the export then missed would be worse than one that never claimed it). **The step
  appears only when the two scopes differ**: an uncapped result *is* every row, so offering to fetch
  them again would be a choice between a thing and itself, and a result with no statement to re-run
  has only the honest scope left. **"Something to re-run" already includes "may be re-run"**: the
  write guard is *inside* `filter::rerun_statement`, which `current_statement` wraps, not a second
  term ANDed on here — the term this menu used to carry could be deleted with the whole suite green
  (see the write-guard invariant). It is a refusal by omission: the scope is simply absent, so it
  cannot be asked for. `All rows` is
  `ExportScope::AllRows`, which carries the **statement** rather than the table — a filtered or
  sorted grid exports all of what it is showing, and a table tab's statement is `SELECT * FROM t`
  anyway — plus the *result's* `database` and `GridState::conn_at_load`, so **both halves of "where
  did this result come from" answer as of the same moment**. The `conn_id` used to come off the live
  tab while the `database` deliberately came off the result, and a tab rebound to another connection
  keeps its loaded result on screen: the two then disagreed and the export could re-run the
  statement against a different server. `conn_at_load` is snapshotted in `GridState::new` from the
  same `gctx.conn_id.get_untracked()` the format seeding already reads. It is
  deliberately a **second read** of the server rather than a continuation of the first: the rows on
  screen may be minutes old, and stitching a stale page onto fresh ones would be neither. The
  statement is snapshotted before the save dialog opens for the same reason the rows are — the
  dialog is modal and slow, and a filter typed while it stood open must not change what the export
  was asked for.
  **Every way the file will differ from the grid is declared at the point of choice, not discovered
  in the file**, and the label is `export::all_rows_label(size, sorted, manual_tx)` — one tested
  function in place of the `match` the menu used to hold, which had none. Two differences, neither of
  them visible in the result. A **client-side sort**: `Fetched` honours `gs.order`, while `AllRows`
  streams the *server's* order, because a column-header sort is a permutation of the rows in hand
  (`compute_order`) that no re-run reproduces — the menu had presented the two as one export at two
  sizes. And a **manual-transaction tab**: `AllRows` is a second read on a fresh connection
  (`Db::stream_query`, deliberately outside the tab's pinned session), so a `TxMode::Manual` tab's
  uncommitted rows are on screen and absent from the file, and rows it deleted are in the file and
  gone from the screen. So the label reads `All rows (~16k, server order)`,
  `All rows (~16k, committed rows only)`, `All rows (server order)` where there is no estimate, or
  plain `All rows` when neither applies. `size` is pre-rendered by the caller, since the estimate and
  its `~` belong to the stats line's vocabulary rather than to this decision; `sorted` comes in as a
  parameter of `export_menu` rather than off `GridState`, because the sort does not
  live there — it is `grid_view`'s, threaded to the header and the toolbar — and `manual_tx` reaches
  it through the `tx_mode` signal `GridCtx`/`GridState` gained for exactly this.
- **A running export is the bottom bar's fourth state, and its Cancel outlives the result.** Only a
  *streamed* export raises it — a snapshot of rows already in memory is written faster than a notice
  about it would be read — and `BarState::Exporting` draws `Exporting… Cancel` (`export_bar`) on the
  note's own fill, with **Cancel** at the right in `theme::err_fix_btn()`, the bar's action colour
  that the error surface's **View** and the wait note's **Rollback** already use. **It began on the
  stats line and moved**, which is worth writing down because it is the kind of thing that gets
  tidied back: that line is the result's own and is the crowded end of the panel — a capped result
  already spends it on `db · 1k of ~16k rows · read 5k rows`, and a fourth clause pushed the whole
  line toward the toolbar icons. The bar is empty almost always, is the full width of the grid, and
  **already owns the report this turns into**: the same strip now says `Exporting… Cancel` and then
  `Exported 16k rows to employees.csv`, so one operation occupies one place from beginning to end
  instead of announcing itself in one surface and reporting in another. It sits **above `Note` and
  below `Error`/`Wait`** in `grid_error_bar`'s chain — a running export is not a problem, so an error
  or a stalled write outranks it, but it *is* live where the note it will become is not, and a
  leftover note must not sit on top of a Cancel the user may still need. The same ordering is why
  `save_export` takes down the bar's *stale* messages before setting the flag: `Exporting` outranks a
  note but not an error, and a failure left standing would otherwise hide the Cancel for as long as
  the export ran. That is **two clears, not one** — `GridState::clear_bar` covers the commit pair,
  and the filter/sort error is the third dismissible message and needs saying separately under its
  own `is_some` guard, exactly the way `dismiss_overlays` already pairs the two. `clear_bar` is
  deliberately not widened to swallow `view_err`: its contract is the commit pair and it runs on the
  keystroke paths, where the guard exists to keep a bar that is already down from invalidating its
  container. The other two bar states are left standing on purpose, and that is the interesting
  half. A stalled write's note (`commit_wait`) is **live, not stale** — the Rollback it offers is the
  more urgent action, so it may sit on top of a running export for as long as the write is out. A
  statement's own failure means **no grid is mounted** to export from, so it cannot coincide —
  and it is not this bar's message any more anyway: `batch_err` is gone, and a statement failure
  goes to the editor's error bar, under the SQL that produced it and beside the Explain and AI fix
  that act on it. (Which is why a failure now *un-collapses* the editor: `Collapse the editor` is a
  button in the RESULTS pane and it sets that pane to height 0, so the message, **View**, **AI
  fix** and **Explain** were all unreachable for exactly the run that needed them. Revealing the
  bar in place is not available — a child overflowing out of a zero-height parent is painted and
  never hit-tested.) `GridCtx::exporting` / `GridState::exporting` is `Option<grid::ExportRun>` —
  `{ id, tab }` — and it answers two questions that used to be one `bool`, each of which had its own
  failure. **`tab` is why the flag may outlive a tab switch.** The export's cancellation token is
  app-global (`main::export_token`), so a flag with a shorter life than the token is a Cancel that
  can vanish while the thing it stops is still running: created per active-tab render, it was
  disposed on switching away and rebuilt empty on switching back, and the stream ran on for minutes
  with no control bound to it and every further `All rows` request refused. The signal is created
  **once for the window, in `center` beside `export_cancel`**, the action that stops it, and the
  launching tab rides *in* the value, which is what keeps the bar off a tab that had nothing to do
  with it: `BarSignals` carries `tab_id` and every reader asks `exporting_here()` rather than asking
  the window. **`id` is why only the export that raised the flag may lower it.** The tail of a
  finished request used to clear it under the same `streaming` test that decides whether to raise
  one — which is true of a second `All rows` request, the only kind the app refuses, so asking for
  one while a stream ran tore down the running export's Cancel. `grid::export_finished(current,
  finished)` clears only when the run reporting *is* the run the flag holds and otherwise leaves it
  exactly as it was, which covers that case and the `Fetched` save that raised no flag at all. The
  ids are monotonic per process, and a counter rather than a token clone because comparison is all
  that is wanted and `ExportRun` has to stay `Copy` for `BarSignals`' sake.
  `export_cancel` is a `TabsActions` action; the app owns the
  token, as it does for query runs and imports. The report
  is an `ExportOutcome` — `Done(export::ExportTally)`, `Cancelled`, `Failed { message, partial }` —
  three outcomes and not a
  `Result` because "the user stopped it" is neither: a red bar for a deliberate Cancel, or a
  cheerful row count for a file that stops halfway, are both lies. It mirrors `ImportOutcome`.
  **`Done` carries a tally and not a row count**, because two kinds of loss are invisible in the file
  itself: `withheld` is the binary columns whose `<n bytes>` placeholder was dropped rather than
  written as data (empty for Markdown and HTML, which keep it deliberately) and `blanked` is
  `ResultSet::capped_columns`, the cells past a column's 512 MiB text arena, which read back as the
  empty string. Nothing had ever read that flag, so a streamed chunk that overran the arena wrote a
  file with holes in it and reported a full row count. The sentence on the bar is
  `export::export_note(tally, name, streaming)`, whose rule is that **silence is the default and a
  caveat overrides it**: a clean `Fetched` save says nothing, since the screen already shows what it
  wrote and a note on every save is a note nobody reads; a streamed one always announces its count;
  and a *loss* speaks in either scope, naming the columns, because a user comparing the file to the
  screen needs to know which part of it to distrust. The count is printed through
  `text::human_count`, the same row-count printer the
  stats line uses, so the figure in the report agrees with the `1k of ~16k rows` directly above it —
  and the note shipped once as `Exported rows to employees.csv`, with no count at all, because
  `text::plural` returns the noun and nothing else and this call site simply never rendered the
  number. **The destination is not opened until the export has succeeded.** Both scopes write a
  `{name}.part` sibling (`export::part_path`) and `std::fs::rename` it over the target at the end,
  the dance `persist` already does for every config file and atomic *because* it is a sibling — a
  rename inside one directory never crosses a filesystem. It used to open the destination first, and
  `File::create` truncates, so a stream that died ten minutes in had already destroyed whatever the
  user was overwriting and all the bar could do was say so. The suffix is visible and
  self-describing rather than a hidden temp name, because when an export does fail the fragment is
  the one thing the user may still want: it is left behind and named in the message rather than
  swept away, deleting it being the one irreversible thing this path could do. **The streaming
  writer holds the cancellation token and refuses to publish when it is set** — a cancel reaches the
  writer as an ordinary end of stream, so publishing first and letting the reader declare the cancel
  afterwards would rename a truncated file over the user's own. Both notes say the two facts that
  follow from all of this: cancel is `Export cancelled — <name> was not changed; the rows that were
  written are in <name>.part` (`export_cancel_note`), and `export_failure_note` says the same in the
  failure's voice, where it previously said `<name> is incomplete` — true when the destination was
  truncated at `t = 0` and now the opposite of true. `Failed` carries `partial`, and `None` is not a
  formality: an export refused *before* the write starts (no connection, one already running) must
  not mention a file at all.
- **The results strip shrinks its prose, never its controls.** It is one flex row — stats, the
  read-more offer, the two red notes, a spacer, then the commit/row/AI/copy/save cluster — and a
  flex row squeezes its children before it overflows, so whichever child refuses to shrink is the
  one that wins. The description lost that fight by default: at a 200k cap on a ~292k table the
  line was long enough to push the icon cluster off the right edge of a narrow panel, and the user
  saw a sentence where the export button should be. The rule is now explicit — `min_width(0)` +
  `text_ellipsis` on the stats label and on both warnings, `flex_shrink(0)` on the icon cluster and
  on the read-more label. A control clipped in half is not a control, while a description ending in
  `…` is still one; and the read-more offer counts as a control, not as prose, even though it is
  made of words. The `dyn_container` wrapping the sort caveat needs the `min_width(0)` too — a
  wrapper sitting at its min-content width holds the text at full size however the child is styled.
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
- **Type-aware headers** show `type_name` under the name (two-line, `grid_header_h()`). A sorted column's
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
