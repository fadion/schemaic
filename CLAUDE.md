# Schemaic

A native SQL editor (Rust + [Floem](https://github.com/lapce/floem) 0.2.0), MySQL/MariaDB-first,
Zed-inspired, aiming to replace DataGrip.

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
    read-only (it can't round-trip through text). The write path's shared decisions live here so the
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
  - `sql.rs` — one `skip_noncode` tokenizer → statement splitting, unsafe-statement guard, AI
    read-only gate, `edit_distance`. The *single* SQL boundary lexer; `intel` (scope/context/
    diagnostics)/`sql_highlight`/`sqlfmt` all build on it so string/`#`/`--`/`/* */`/backtick
    boundaries agree by construction.
  - `intel.rs` — the **SQL intelligence** layer (structure-aware, dialect-pluggable). Parses a
    *complete* statement with a real per-dialect AST (`sqlparser`; `SqlDialect` seam — MySQL wired,
    Postgres/SQLite are future arms) and answers what a token stream can't: `statement_scope`
    (tables/aliases/CTEs/derived-tables in scope, AST-backed with a `skip_noncode` lexer fallback for
    mid-edit), `clause_context`/`clause_continuation` (caret context + expected-token model for
    completion), and `diagnostics` → `Vec<Diagnostic>` (catalog-aware unknown-table, unknown-column via
    the per-scope resolver `colres` — qualified *and* unqualified, across subqueries/derived-tables/CTEs
    with correlation — reserved-keyword-alias errors, syntax errors on completed statements, and
    keyword-typo warnings). **AST for classification, `skip_noncode` for byte positions by default** —
    except `colres`, which uses sqlparser 0.62's now-accurate per-identifier *spans* (verified) so the
    same column name in an inner vs outer scope is placed independently. `Catalog` is the case-folded view over
    the introspected `DbSchema`s (columns + FK edges); building one is O(tables × columns) and a keystroke asks
    for it up to four times, so the UI goes through `CatalogCache` — a memo keyed on the **`Arc<DbSchema>`
    identity** of the schemas it was built from rather than a hand-bumped generation, so a re-introspection
    misses by construction and there is no write site to remember. Hosts the shared `SQL_KEYWORDS`/`SQL_FUNCTIONS`/
    `STMT_KEYWORDS` (the UI's completion + editor build on these). Also `join_condition` (FK-aware
    `JOIN … ON` auto-fill), `db_error_diagnostic` (positions a live DB error within the statement),
    `parses` (Tier-2 gate), and `select_output_names` (a projection's column names *in order*, or
    `None` when the statement alone can't say — what `ddl::pg_replaceable` reads).
    **Every SQL fragment this module generates** (`join_condition`, `join_targets`, `expand_star`)
    is quoted through `export::ident_if_needed`, which quotes only what a bare name would get wrong
    — anything that isn't a plain lower-case non-reserved word. PostgreSQL folds an unquoted
    `ArtistId` to `artistid`, so unquoted output couldn't run on any mixed-case schema, while
    unconditional quoting would backtick every ordinary MySQL name in text the user is about to
    edit. `JoinTarget` therefore carries the **bare** name for the popup to display and prefix-match
    and `table_sql` for what is actually inserted. The live DB stays the semantic authority: **Tier-2 live validation**
    PREPAREs the statement under the cursor (`Db::prepare_check`, non-executing) behind the
    `live_validate` setting (off by default), merging dialect-exact errors into the editor squiggles.
  - `filter.rs` — the header filter/sort bar: a dialect-aware `sqlparser` **AST rewrite** that
    splices a `WHERE`/`ORDER BY` into the `SELECT` that produced the result and hands back SQL to
    re-run — so filtering covers the whole table, not the loaded page. `build_query` rewrites only
    a structurally simple, join-free, CTE-free single-table `SELECT` and degrades to `Ok(None)`
    ("not filterable") rather than erroring, because eligibility is the caller's question.
    `eq_condition` is the right-click "Filter by / Exclude" fragment. `table_query` is what opening
    a table from the tree generates: it orders by the PK on purpose — neither engine promises row
    order for a capped page, and PG heap order shifts under an `UPDATE` — while leaving an ordinary
    name unquoted, since a quoted identifier blinds the mid-edit tokenizer behind completion.
    Quoting here goes through the same rules as the rest of the app; don't add a fourth.
  - `edit.rs` — `analyze_edit` → `EditModel` (write-back updatability analysis) + `refetch_template`
    and `refetch_key`, the **one** post-edit re-fetch key builder. A key column *is* editable
    (`EditModel::editable` asks only whether a column maps to a base table), so the `UPDATE` keys on
    the original row while the re-fetch must look for the value it just wrote; there were two
    builders and only one knew that.
  - `export.rs` — CSV/JSON/SQL/Markdown/HTML export (incl. CSV formula-injection guard;
    Markdown pipe/backslash escaping; HTML entity escaping). Every renderer has a **streaming**
    `*_to<W: io::Write>` form (`ExportFormat::render_to`) — what file export uses, so a large
    result is never rendered into a second full copy in memory — with the `String` versions kept
    as thin wrappers for the clipboard. A test asserts the two agree byte-for-byte per format;
    add new formats to both by adding the `*_to` and wrapping it.
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
    says otherwise. Pure + unit-tested.
  - `ddl.rs` — **schema editing**. `TableDraft` (the desired table; column/index/FK
    entries each carry the name they had on the server, which is what tells a *rename*
    from a drop-plus-add) → `diff(current, draft, dialect) -> ChangeSet` → `emit()`.
    Every `Change` answers `summary()` and `risks()`, which is what the preview
    modal renders — `risks()` returns *every* consequence, not the first, because one edit can
    narrow a column **and** make it NOT NULL and the NOT-NULL sentence ("the statement fails")
    otherwise reads as a promise that nothing is lost. The emitter owns the engine divergence: MySQL coalesces into one
    `ALTER TABLE` and restates a whole column via `definition_sql` (`MODIFY` replaces it,
    so anything omitted is destroyed); PostgreSQL splits renames / `DROP INDEX` /
    `CREATE INDEX` / `COMMENT ON` into their own statements and drops a key by
    *constraint* name (`IndexInfo::constraint` — it has no `DROP PRIMARY KEY`). Ordering
    is dependency-first (FKs and indexes off before the columns under them; keys back on
    after). `normalize_type`/`types_equal` + `defaults_equal` are the reason a designer
    opens clean — `int(11)` ≡ `int`, `character varying(45)` ≡ `varchar(45)`. **The
    round-trip gate is test-enforced**: `TableDraft::from_table(t)` diffed against `t`
    must be empty over captured fixtures from classicmodels/sakila/employees/world +
    PG world/chinook (`ddl::tests::roundtrip`) — extend those fixtures rather than
    working around them, since any model-fidelity gap surfaces to the user as a phantom
    change. Also `key_list_text`/`parse_key_list` (the designer's `bio(20), age DESC`
    field) and `common_types`. Pure + unit-tested.
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
    → the same preview. Two engine rules live here. MySQL's `CREATE OR REPLACE VIEW`
    replaces the *whole* view, so the emitter restates `ALGORITHM`/`DEFINER`/
    `SQL SECURITY`/`CHECK OPTION` — omitting the security type silently turns a
    `DEFINER` view into an `INVOKER` one, which is a privilege change, the same class
    of bug as `MODIFY COLUMN`'s. PostgreSQL's may only **append** columns, so an edit
    that renames/retypes/reorders one needs `DROP` + `CREATE`, which takes dependent
    views and grants with it: `pg_replaceable` (over `intel::select_output_names`)
    decides where it can, **uncertainty resolves to replace-and-let-the-server-refuse,
    never to drop**, and `ViewDraft::force_recreate` is the user's override.
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
    **Triggers and PostgreSQL trigger functions** ride the same rails again:
    `TriggerSetDraft`/`TriggerDraft` → `diff_triggers` and `FunctionDraft` → `diff_function`
    → `Change::{CreateTrigger, ReplaceTrigger, DropTrigger, CreateFunction, ReplaceFunction,
    RenameFunction, DropFunction}` → the same preview. Neither engine can *alter* a trigger,
    so **every** edit is a drop-and-create and `ReplaceTrigger` is that pair — which is why
    `trigger_statements` emits **all the drops, then all the creates** rather than each pair
    together: adjacent pairs collide the moment two triggers swap names, and on MySQL
    statement 1 has already committed when statement 2 fails, so the first trigger is simply
    gone. Same rule, same reason as `GridWrite::plan` in `core::model`.
    `session_wrapped_create` is the MySQL half of that emitter: `CREATE TRIGGER` has no clause
    for the `sql_mode`/`character_set_client`/`collation_connection` a trigger was written
    under, yet all three are part of what it does, so the values are set on the session around
    the statement and restored after (`run_ddl` runs a MySQL plan in order on one connection,
    which is what makes that safe). Nothing is emitted when nothing is known — `None` means
    "not fetched", and inventing a session state is a change nobody asked for.
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
  - `monitor.rs` — the **Live Monitor**'s pure change detector: no DB, no timer, no UI.
    `Snapshot::from_result` captures a `ResultSet` keyed by its table's key columns (cells are
    `Option<String>` so NULL stays distinct from `""`), and `diff_snapshots` matches two snapshots
    by key into `RowChange::{Insert,Update,Delete}`, an update carrying per-column `FieldChange`.
    Row identity is just `Vec<String>`, so a new engine's fetch path only has to produce a
    `Snapshot`. The caller must skip the *first* poll itself — diffing against an empty prior reads
    every row as an insert. A delete carries the row's last-seen cells deliberately: it is the one
    case where the row is gone from the database and the log is the only remaining record.
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
    and a test pins the two together.
  - `health.rs` — connection health-poll policy: `tick(HealthCfg, TickCtx) -> Tick` decides
    ping-or-skip + the delay until the next tick (exponential `backoff` on consecutive failures,
    longer interval for SSH-tunnelled connections, skip while the window is unfocused / a query is
    already in flight / the tunnel isn't up). The app owns only the timer + `Db::ping`.
  - `tx.rs` — the **manual-transaction** state machine behind `TxMode::Manual` (no DB, no UI).
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
    database — the sidebar lists all of its databases. `SshTunnel`/`SshAuth` cover the tunnel's own
    auth, including `Agent` (delegates to the running SSH agent, storing no secret at all).
    `ConnStatus::is_down` treats `Unknown` (not yet checked, or a tunnel still coming up) as
    *non*-blocking — only a confirmed failure gates work. `SshAuth`/`Environment` deserialize
    through a `…Raw` shim with `#[serde(other)]`, so a value written by a newer build degrades to a
    default instead of failing all of `connections.json`. There is deliberately no
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
    two halves of a MySQL account. `TableInfo::create_ddl` — `CREATE TABLE`/`VIEW`, built on the
    above; its **view** branch delegates to `ddl::view_ddl` so Copy DDL, the MCP table-info tool
    and the apply path all emit through one view emitter (it used to have its own, which restated
    none of the options).
    **`TriggerInfo`/`TriggerAction`/`TriggerEvent`/`TriggerEnabled`/`TriggerSource` +
    `RoutineInfo`** are the trigger and PG-trigger-function half, and carry three rules the
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
    `CheckInfo::validated`/`inherited` are PostgreSQL's `NOT VALID` / `NO INHERIT`, carried and
    restated: they are part of the clause, and `pg_get_constraintdef` prints them *after* the
    parens, which is why `ddl::check_predicate` must strip them before peeling.
    **The standalone PostgreSQL objects** — `EnumInfo`/`DomainInfo`/`SequenceInfo` — sit here
    beside the tables and, unlike `RoutineInfo`, **on `DbSchema` itself** rather than being
    fetched lazily: the tree lists them and a column's type *is* one of them, so a separately
    refreshed second cache would be a second answer to "what is in this database" and the two
    would diverge on the first refresh that only updated one. An enum's `values` are in
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
    gets wrong.
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
      built from server-controlled text goes through it.
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
    - `transcript.rs` — the rendered shape of one AI turn (`ChatMessage`/`Seg::{Text,Tool}`/
      `TurnStats`), kept here rather than in `schemaic-ai` so the UI crate needn't depend on the
      CLI-integration crate. `ChatMessage::prose` is what copy *and* conversation replay use.
      Also the message box's **prompt recall** (Ctrl+Up/Down): `user_prompts` (the user's own
      questions, newest first, blanks dropped and a repeat kept only at its newest spot) +
      `recall_step`, which is a **cycle** — `None → newest → … → oldest → None` — rather than a
      list that stops at its ends, because the empty box is the only way back out of a recall and
      both keys have to reach it. A cursor past the end of a conversation that has since changed
      reads as `None`, so a stale walk restarts instead of landing somewhere arbitrary.
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
  - **Small persisted / UI-state models**, each a flat `Vec` keyed by `conn_id` and each pure +
    tested (they share `history.rs`'s shape; a new one belongs here, not in the UI):
    - `search_history.rs` — recent Find-Anywhere targets (`MAX_PER_CONN`, newest-first, deduped).
      `push` records only an *activated* result, not every keystroke, and the PG namespace is part
      of the dedup identity so same-named tables in two schemas don't collapse into one. So is
      `ObjectTag`, which is what makes an entry a PostgreSQL enum/domain/sequence rather than a
      table (its name rides in `table`, so every file written before objects were searchable still
      loads): a type and a table may share a name in one namespace and are different places to go
      back to. The tag is a **persisted** enum of its own rather than `ddl::ObjectKind` so a kind
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
    - `db_color.rs` — a per-`(connection, database)` identity colour. Display-only (a dot in the
      tree, the active-DB selector and tabs) and **manual only** — never inferred, and explicitly
      not the editor's production-red danger frame, which stays a *connection*-level signal.
    - `tabsel.rs` — tab-selection rules for a strip that shows only the active connection's tabs, so
      every question (`pick_active`, `neighbor` after a close, `cycle`, `closing_would_empty`, `nth`
      for Ctrl+1‑9) is answered *within one connection*. `nth` especially: the Nth visible chip is
      not the Nth entry of the flat `Vec` once another connection's tabs interleave.
      `pick_active` prefers the remembered per-connection tab, so switching away and back doesn't
      dump the user on tab 1.
    - `palette.rs` — parses the command palette's `>` command mode into
      `Parsed::{Search,Filter,Command{name,arg}}`. The hard part is when typing stops filtering the
      command list and becomes an argument: longest-word-prefix match against the caller's
      argument-command names, under an invariant the caller must uphold — no argument-command name
      may be a word-prefix of another (`indent style`/`indent width`, never a bare `indent`).
    - `resource.rs` — the status bar's CPU/RAM model. `ResourceSample::new` divides `sysinfo`'s
      per-process CPU% (single-core-relative, so it exceeds 100 on a multi-core box) across the
      logical core count to give a whole-machine 0..=100. Sampling itself stays at the app boundary.
    - `text.rs` — `plural(n, one, many)`, returning only the noun form so a humanized count
      (`"1.2k"`) can be displayed while the singular/plural decision still follows the true `n`.
- `schemaic-db` — MySQL/MariaDB (`mysql_async`) + SSH tunnels (`ssh.rs`), PostgreSQL in `pg.rs`, and
  the pinned manual-transaction connection in `session.rs`. Populates each result column's
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
  (non-executing `PREPARE` for the editor's live validation)/`run_ddl` are `Db` methods taking
  the target DB per call. `run_ddl` is the schema-editing apply path and is **honest about
  atomicity**: PostgreSQL runs the whole plan in one transaction (transactional DDL), MySQL runs
  it sequentially and reports which statement failed *and how many already stuck*
  (`DdlError::applied`) — every MySQL DDL statement commits implicitly, so a transaction there
  would be theatre.
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
  On the PG side, `pg_triggers` filters `tgparentid = 0` (a partition's cloned trigger is
  `tgisinternal = false` and can only be dropped through its parent), and `UPDATE OF` columns +
  a function's `proconfig` arrive **one row each** rather than string-aggregated — same rule the
  enum labels follow, and for the same reason.
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
    `section_title`/`centered_msg`/`toggle_icon`, `measure_text_px`, `jump_to_bottom_button`.
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
    `focus_root_with_ring`/`innermost_focus_ring` (how the modal root and the *window* root enter
    it), `in_focus_ring`/`in_focus_ring_with` (how a non-field control joins, the second for one
    with teardown of its own — floem keeps a single cleanup slot), `VALUE_TAB` (where a growing
    block of stops starts), and the `PopupToken`-tagged `set_open_popup`/`clear_open_popup`/
    `dismiss_open_popup` slot. A field joins through `FieldCfg::focus` instead, since nothing
    outside floem's editor can see a key it has.
  - `markdown.rs` — AI-chat `render_markdown`/`CodeActions`/`code_block` (pulldown-cmark).
  - `settings.rs` — the three settings modals **and the four shared controls every modal's form is
    built from**: `focusable_toggle`/`focusable_toggle_row` (the switch — Space is ours, Enter is
    floem's), `focusable_dropdown` and the picker-agnostic `in_ring_dropdown` under it (which owns
    the four floem work-arounds a keyboard-operable dropdown needs). `themed_toggle` and
    `settings_dropdown` are the un-ringed builders beneath, and are **private** on purpose: a
    control nobody can Tab to is one left out of the modal's keyboard order by accident.
  - `shortcuts.rs` — the app's keyboard shortcuts as **one table** (`SHORTCUTS`), which
    `settings::help_overlay` renders straight from — plus the tests that keep it honest. This list
    is the app's *only* keyboard documentation and for Ctrl+H / Ctrl+G the only affordance of any
    kind, so a binding missing from it is a feature nobody can find; it was a literal inside the
    modal and drifted exactly as its own comment predicted, hiding Alt+↑/↓, Ctrl+↑/↓, Ctrl+Shift+C/V
    and Ctrl+Home/End. **The handlers can't render from the table** — they are `match` arms on
    `Key::Character` across four files — so the guarantee runs the other way: the tests scan those
    files for the four idioms the codebase binds a **Ctrl/Alt + letter** with (the `"x" | "X"` case
    pair, `eq_ignore_ascii_case`, `NavKeys`' `Some("x") =>`, and `KeyCode::KeyX` for the physical
    match Ctrl+Alt+L needs) and fail when one has no row, with `EXEMPT` the justified-baseline
    escape hatch in the spirit of `contrast::UI_SHORTFALL`. Deliberately **weak**, like
    `doc_coverage`: it catches the binding nobody wrote down, not an inaccurate row. Two rules
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
    than none: it teaches a key that does something else. The string must be **byte-identical** to a
    `SHORTCUTS` row (tested), so the palette can't advertise what the modal doesn't document; the
    other half — that each name still names a live command — can only be checked against a built
    registry, so it rides `overlays::assert_names_match_labels`' `debug_assert`, without which a
    renamed command would drop its keycap in silence.
  - `connection_form.rs` — Manage Connections modal + password-mask (+ tests).
  - `diff_view.rs` — Ctrl+K diff preview. `history_panel.rs` — Query History right-column panel.
  - `plan_view.rs` — Query Plan modal (`EXPLAIN`/`EXPLAIN ANALYZE` table + warnings + "Ask AI"),
    via `TabsActions::run_plan` → `Db::explain`.
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
    **Every path ends at `ddl_preview`** — designer, Create table, and the context-menu
    shortcuts — so there's one place that shows the SQL, one that names what's destroyed, and
    one "Open in editor" escape hatch. Never run generated DDL without it. Entry points:
    `table_designer::open_for_table`/`open_for_new`/`preview_draft_edit` (a shortcut whose
    edit has dependents — dropping a column takes its index and FK with it) and
    `ddl_preview::preview_change` (a lone `Change`).
  - `view_editor.rs` — the **view** modal (tree "Edit" on a view, "Create view" on a database/
    schema node *and* on the editor's right-click when the statement under the caret can be a
    view body — `ddl::can_be_view_body`, which seeds the draft with it), over `core::ddl`'s
    `ViewDraft`. Not a designer tab: a view is a name and a
    `SELECT`, so it's one form on the shared modal chrome, ending at the same `ddl_preview`.
    Same seed-local-signals-then-write-back rule as the designer (the form is built once per
    open; only the footer is keyed on the draft). The options are shown because they're
    *carried* through a replace, and the PG "re-create instead of replacing" toggle is the
    override for the cases `ddl::pg_replaceable` can't read off the statement.
    `is_editable_view` is the entry point's gate — a materialized view is drop-only.
  - `trigger_editor.rs` — the **trigger** modal *and* the **function** modal, over `core::ddl`'s
    `TriggerSetDraft`/`FunctionDraft`. Reached from the schema context menu's per-table
    **Triggers…** entry; same chrome, same seed-local-signals-then-write-back rule and same
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
    Two modals in one module because the second only exists to serve the first: a PG trigger has
    no body, only a **function** to call, so the trigger form would be a dead end without a way to
    write one. Three rules are written down because each was a bug waiting: **the form is
    per-engine because the objects are** (MySQL owns a body and one event; PG calls a function,
    takes several events and a `WHEN`), so it *hides* what an engine can't express rather than
    offering it and failing at apply; **the function list is fetched lazily** (`Db::trigger_functions`
    via `TriggerFnFn`, the same call `view_algorithm` makes) and arrives a round trip late, so the
    picker keeps whatever the draft already names instead of selecting the first entry and silently
    re-pointing the trigger; and **the trigger target is never cleared while the function modal is
    up** — its overlay just renders nothing — so closing that one reveals the half-filled trigger
    form intact, with no "return to trigger" flag to be a second source of truth. `is_editable_trigger`
    is the entry point's gate: a constraint trigger's deferral settings aren't modelled, so it is
    listed and droppable but not editable, the call a materialized view gets.
  - `object_editor.rs` — the **enum / domain / sequence** modal, over `core::ddl`'s
    `ObjectDraft`. Reached from a tree object's **Edit** and from a database or schema node's
    **Create ▸ Type / Domain / Sequence** (PostgreSQL only — on MySQL those entries don't
    exist, the same "hide what an engine can't express" call `trigger_editor`'s form makes;
    `overlays::create_submenu` is the one builder both nodes' Create submenu comes from).
    One modal for three objects because the chrome, the footer, the change count and the
    ending at `ddl_preview` are identical and only the middle section differs. Same
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
    `assistant_footer`).
  - `overlays.rs` — absolutely-positioned popups: connection/active-db/schema menus, schema context
    menu, generic grid popup, Find-Anywhere, error modal.
  - `schema_tree.rs` — SCHEMA sidebar (`schema_panel` + db/table/column/key row builders + keyboard
    nav). PostgreSQL's standalone objects hang off the same levels the tables do, in
    `Types`/`Domains`/`Sequences` folders after them (`object_groups`/`object_group_node`/
    `object_row`, over `schema::ObjectItem`); an empty folder isn't rendered, and none of them
    exist on MySQL, so that tree is untouched. They are scoped by `TableScope` for the reason
    it exists — *flat* means the database has no schema level, not that its objects have no
    namespace. Two filter rules follow from the level above being evaluated first: a database
    and a namespace both survive a search that only one of their **objects** matches, or the
    match would be hidden by the row that contains it. `nav_rows` carries the folders and their
    leaves like everything else — it is the function that must stay bug-for-bug identical to
    the render. `completion.rs` — SQL autocomplete: the ranking + popup layer
    (`recompute_completions`/`accept_completion`/`completion_popup` + `SchemaIndex`/`fuzzy_score`)
    over `schemaic_core::intel`'s scope/context engine.
  - `tabs.rs` — query-tab strip. `grid.rs` — the whole results grid (`GridState`/`GridCtx`;
    `results_view`/`loaded_view` are the entry points). `editor_pane.rs` — SQL editor pane
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
  - `monitor_view.rs` — the **Live Monitor** modal (`monitor_overlay`), opened from the results
    title bar with the tab's `(conn_id, database, table)`. It renders `overlay.monitor_log` — built
    by the app's poll loop through `core::monitor::diff_snapshots` — as a Time·Action·ID·Data table,
    and owns *none* of the polling: closing the modal flips `overlay.monitor_open` false, and that
    is what stops the loop.
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
  - `lib.rs` (~5.6k lines; `grid.rs` at ~6.3k is the crate's largest) — the `Ui` struct + bundles, shared model/state
    types, `workspace`/`body`/`center`/`header`/`footer`, resize handles, `edit_field`/`FieldCfg`,
    terminal panel. The shared types living in the crate root is what stalls further splitting: the
    root depends on the leaves (`mod`) and the leaves depend on the root (types), so a view builder
    can't move out until the types do.
- `schemaic-app` — `main.rs` wires signals + callbacks and builds the `Ui`; also the built-in MCP
  server (`--mcp-serve`) the AI panel talks to. A query tab's identity is `(conn_id, database)`;
  the app resolves `conn_id` → `Db` at run time (`db_for`), so a tab keeps its connection after a
  switch. The MCP subprocess gets its DB endpoint as JSON in `$SCHEMAIC_MCP_ENDPOINT` via a
  per-session temp `--mcp-config` file (removed on drop) — never argv, so credentials don't leak
  to other same-user processes. Pure clusters split out: `claude_cli.rs` (`claude` binary
  discovery — PATH/PATHEXT/override) and `ai.rs` (`AiSession`/`start_ai_session` streaming,
  MCP-config plumbing, `ai_context`/`inline_system_prompt`). Reactive wiring (`app_view` closures)
  stays in `main.rs`. The MCP server itself is `mcp.rs`; `secrets.rs` is the keyring-backed
  `SecretStore` behind `core::secrets`.
  - `heap.rs` — process-wide heap accounting. `Tracking` is installed as the global allocator and
    adds only two atomics — **live** bytes (allocated − freed) and the running peak — over the
    system allocator. It exists to answer one question the OS can't: whether memory growth is a
    real leak or benign allocator/OS retention. Live returning to its baseline after a table closes
    while the working set stays high is the allocator holding freed pages for reuse; live *not*
    returning is the leak.

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
- **One SQL boundary lexer.** Any code scanning SQL for string / `-- ` / `#` / `/* */` / backtick /
  `$tag$` boundaries MUST build on `schemaic_core::sql::skip_noncode` (statement split, WHERE guard, AI
  read-only gate, `intel`'s tokenizer, `sql_highlight`, `sqlfmt`). Never hand-roll a second
  scanner — five drifting copies was the original bug. **It's dialect-aware:** `skip_noncode`/
  `skip_comment` (and the `sql.rs` helpers built on them — `statement_bounds`/`ranges`/`range`,
  `read_only_reason`, `has_top_level_where`/`unsafe_reason`/`first_unsafe`/`contains_write`) take a
  `SqlDialect`, so pass the connection's dialect (Postgres `#` is an operator not a comment, `$tag$…$tag$`
  is a string, `"…"` an identifier, `\`-escapes only in MySQL / PG `E'…'`). **No exceptions** —
  `intel::tokenize_range` (the mid-edit byte-position *fallback*) is dialect-aware too, and so are the
  `intel` entry points that reach it (`clause_context`/`clause_continuation`/`join_targets`/
  `expand_star`/`signature_help` all take a `SqlDialect`). It additionally lifts a **quoted identifier**
  out as a word — `` `t` `` on MySQL, `"t"` on PG — since that's the form Schemaic itself generates and
  the fallback is exactly what runs mid-`WHERE`.
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
  consequence is stated in plain language and where "Open in editor" hands the script over. A
  new engine is a `SqlDialect` arm in `ddl.rs`'s emitter, not a parallel emitter. The
  round-trip gate (a draft built from a table must diff to *nothing*) is the test that keeps
  the introspected model and the emitter honest with each other; extend its fixtures when you
  widen the model.
- **Write-back is transactional with a 1-row safety net — and the *report* never claims more than
  the engine delivered.** `commit_writes` runs a `GridWrite`
  (DELETEs → UPDATEs → INSERTs) in one transaction, each statement required to affect exactly 1 row
  (else roll back all) — so an over-optimistic updatability analysis can't corrupt data. That
  promise is MySQL-engine-dependent: `MyISAM`/`MEMORY`/`ARCHIVE`/`CSV` ignore `BEGIN`/`ROLLBACK`,
  and `ROLLBACK` *succeeds* there while raising warning 1196. So no write path may discard a
  rollback's outcome (`let _ = conn.query_drop("ROLLBACK")` was the bug): roll back through
  `rollback()`, which reads `SHOW WARNINGS`, and append `core::model::Rollback::note()` to the
  error. `one_row_verdict` states only what the guard saw — it runs *before* the rollback and can't
  know what it achieved. `engine_is_transactional` is the predicate (unknown ⇒ not transactional,
  same rule as `pg_replaceable`); the import modal warns from it before the load starts. Commits
  with inserts/deletes full-re-run the query (membership/order changed); pure-UPDATE commits splice
  in place. Both halves of that rule are **pure and tested in `core::model`**, and both engines'
  executors call them: `GridWrite::plan` is the statement order and `one_row_verdict` is the
  per-statement verdict *and* its message — so neither can drift between MySQL and PostgreSQL, and
  a change to `affected != 1` fails a test rather than passing silently.
- **One identifier quoter, as there is one boundary lexer.** Every path that quotes an identifier
  ends at `export::ident_sql` (unconditional — for SQL that is only executed) or its sibling
  `export::ident_if_needed` (only when a bare name would name something else — for SQL the user
  reads and edits). `filter::quote_ident`, `schema::ddl_ident_in`, `db::pg::pg_ident` and
  `db::ident` are all thin delegations; the two engine-fixed ones in `schemaic-db` are bound by a
  test in that crate, since they can't take a dialect. **Don't write a fifth** — there were four,
  each having independently arrived at the same escaping, which is the drift hazard rather than the
  reassurance: the literal half of the same split (`schema::ddl_string` missing MySQL's backslash
  escaping while `export::sql_literal` had it) shipped as a High.
- **Both schema-search surfaces match through one predicate.** The schema tree's filter box and the
  Find-Anywhere palette answer the same question over the same `DbSchema`, so they go through
  `schema::TableInfo::matches_search` (name or any column) and `schema::ObjectItem::matches_search`
  (name only — a `detail()` match would surface a sequence because some unrelated table's name
  appeared in its owner). They were two predicates and the palette's simply **had no object arm**,
  so on a PostgreSQL connection Ctrl+P for a type you were looking at in the sidebar returned
  nothing. `overlays::schema_hits` is the palette's half split out as plain data for exactly this
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
- **Splitting `lib.rs` / `main.rs`:** grep the line range for interleaved unrelated `fn`s first; a
  helper still used by code that stays goes to `widgets.rs` (glob-imported), not the new leaf
  module; mark cross-called items `pub(crate)`; build + `cargo fmt` + smoke-launch each step.

## Testing (TDD is the default now)

**Test-driven development is the working approach for this project going forward.** New behavior and
bug fixes start with a failing test, then the code that makes it pass. Concretely:

- **Red → green → refactor.** For any new pure-logic behavior or bug fix, write the test first (it
  fails), then implement until it passes, then clean up with the test still green. When a bug is
  reported, first add a test that reproduces it (red), then fix it.
- **Where tests live.** Pure logic belongs in `schemaic-core` (or the owning crate's `src`) with an
  inline `#[cfg(test)] mod tests`; regression tests for a bug live next to the code they guard. The
  UI/app keep thin wrappers over `schemaic-core`, so push logic *down* into a testable core function
  rather than testing it through the UI.
- **Coverage bar.** Every public function that encodes a decision (parsing, analysis, formatting,
  export, diffing, key selection, gating) must have unit tests covering the happy path, empty/edge
  inputs, and known failure modes. Prefer many small, named tests over one broad one.
- **Keep the suite green + fast.** `cargo test --workspace` must pass before any commit; tests stay
  pure (no live DB / network / filesystem — model those at the boundary). Don't commit with failing
  or `#[ignore]`d tests unless the user asks. The single exception is
  `core/tests/doc_coverage.rs`, which asserts every `src/*.rs` module is named somewhere in this
  file — the thing under test *is* a file. A new module fails it until it's on the map above.
- **Architecture invariants are test-enforced where possible** — e.g. the single SQL boundary lexer,
  the 1-row write-back safety net, and edit-model key selection all have regression tests; extend
  them rather than working around them.

## Build & run

- `cargo build` / `cargo run -p schemaic-app`.
- **Windows:** if the app is running, the linker can't overwrite `target/debug/schemaic.exe`
  ("Access is denied"). Stop it first (`Get-Process schemaic | Stop-Process -Force`).

## Never bulk-rewrite source with a script

**Do not use `sed -i`/`awk`/`perl -i` (or any generated script) to edit `.rs` files in place.** Use
the editor tools, one site at a time, driven by the compiler's error list. Adding a field to a
widely-constructed struct breaks 20+ literals and the temptation to "just script it" is exactly
when this goes wrong.

This is written down because it already destroyed ~900 lines across seven files in one command. The
awk had a line-buffering bug: it printed a held line only on *some* paths, so every branch that
didn't print silently dropped a line. The damage isn't localized to the intended matches — it's
scattered through whole files — and it compiles-ish, so the error list *shrinks* and looks like
progress. Recovery was only cheap because the tree happened to be committed a few minutes earlier.

If a mechanical edit really is unavoidable:

- **Commit (or stash) first.** A dirty tree plus an in-place script is how uncommitted work dies.
- Verify with `git diff --stat` **before** trusting the build: net line count should match what you
  intended. Mass deletions are the tell — a shrinking error count is not.
- Prefer a change that avoids the churn (a `Default` derive + `..Default::default()`, a constructor,
  a `From` impl) over a change that requires touching every call site.

Recovery, if it happens anyway: `git show HEAD:<path> > <path>` per file. Plain `git checkout --`
/`git restore` may be blocked as a destructive operation, and `git show` is read-only.

## Commits & releases

- **Never commit unless the user explicitly asks.** Making edits does not imply committing — leave
  changes in the working tree. Same for `git tag`/`git push`. Amending is fine when the user is
  iterating on a commit.
- **Always run `cargo fmt --all` before a push.** CI (`ci.yml`) fails the build on an unformatted
  tree (`cargo fmt --all --check`), and it's historically the most common CI failure. Run it and
  commit any resulting changes *before* `git push` — verify with `cargo fmt --all --check` (exit 0).
- **Conventional Commits** — `type(scope): subject`, imperative, no trailing period, lower-case
  after the colon. Types: `feat`/`fix`/`refactor`/`perf`/`docs`/`test`/`chore`/`build`/`ci`. Scope
  = the crate/module the change centers on (`grid`, `editor`, `schema`, `ai`, `sql`, `theme`, `db`,
  `ci`…); omit only when cross-cutting. Optional body (blank line first) explains the *why*. Every
  message ends with the trailer `Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>`. Example:
  `feat(grid): add row cloning via context menu`.
- **Version bumps are explicit-only.** Bump only when asked; never as a side effect of an unrelated
  commit. Edit **one** place — `[workspace.package].version` in the root `Cargo.toml` (all crates
  inherit via `version.workspace = true`; never a per-crate `version`). Commit as
  `chore: release vX.Y.Z`.
- **Releases are tag-driven.** Bump → commit → `git tag vX.Y.Z && git push origin vX.Y.Z` (keep tag
  and `Cargo.toml` in sync). The tag triggers `release.yml` (Linux + Windows binaries → GitHub
  Release); `ci.yml` runs fmt + clippy (`-D warnings`) + **rustdoc (`RUSTDOCFLAGS=-D warnings cargo
  doc --workspace --no-deps`)** + `cargo deny` + build/test on push/PR. Keep the tree green before
  tagging. The rustdoc gate is the one no local habit runs, and a doc link pointing at a renamed
  item has failed a push on exactly it — in PowerShell that check is
  `$env:RUSTDOCFLAGS = '-D warnings'; cargo doc --workspace --no-deps`, since the POSIX env-var
  prefix is a parse error there.

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
- Small visual tweaks: build only, let the user verify. Screenshot harness for new features /
  behavior debugging, or when asked.

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
- **Absolute overlays** (placeholders, action bars) intercept clicks — add `.pointer_events(|| false)`
  so clicks fall through.
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
  root steps `widgets::innermost_focus_ring()` instead, which is why `FOCUS_ROOTS` carries
  `(ViewId, Option<FocusRing>)` rather than a bare id.
  **Every cleanup that can run while focused hands the keyboard back** — `focus_root`,
  `in_focus_ring` and `edit_field` all do, because floem clears `app_state.focus` *silently* on
  removal (no `focus_changed`, so no `FocusGained` lands anywhere) and the modal around the
  departing control is then deaf to Escape and Tab alike. One click on the designer's list `+` was
  enough. "Was I focused?" is mirrored into an `Rc<Cell<bool>>`, never read back from a signal that
  may already be disposed. Note floem keeps **one** cleanup slot per view, so a control needing its
  own teardown passes it to `widgets::in_focus_ring_with` rather than chaining a second
  `.on_cleanup`, which would silently replace the unregister *and* the hand-back.
- **A `.hide()`n control is still in the Tab order** — `hide()` is `display: none`, so the view is
  still in the tree and still registered in the ring, and Tab moves focus onto something nobody can
  see. Every engine-conditional block that was built-and-hidden is therefore now **built
  conditionally**: import's CSV settings, the designer's MySQL-only engine/collation and `ON UPDATE`
  and PostgreSQL-only index method/predicate, the view editor's MySQL options and PG recreate
  toggle, the trigger form's `Fires`/`When`. Nothing is lost by rebuilding — each of those binds
  straight to a draft or a persisted signal — and a control an engine can't express shouldn't be
  reachable at all, which is the same call `trigger_editor`'s per-engine form already made.
  **The else-arm is still `display:none`, via `widgets::nothing()`** — taffy skips a `display:none`
  child when it distributes `gap` but counts a zero-sized one, so a bare `empty()` arm leaves a
  whole `FORM_GAP` of dead space where the block would have been. The rule is about *controls*: an
  arm with nothing inside it has nothing to be Tab-reachable. Where the conditional is a
  `dyn_container`, the hide goes on the **container** — that is the flex child, not its inner view.
- **Buttons are in the ring too, and Space or Enter presses them — but there is no default Enter.**
  Every button a modal has goes through `widgets::in_ring_button` (which the six builders —
  `action_button`/`action_button_icon`/`action_face`/`control_button`/`control_button_enabled`/
  `row_button` — call for you; the ring parameter is *required*, so a modal button that isn't
  reachable won't compile). Enter in a *field* fires nothing: the DDL preview's Apply is an
  irreversible `ALTER`, and a key meaning "newline" in one control and "apply the plan" in another
  is the shape of defect the ring's own review was full of. **A disabled button is not a stop** —
  it keeps its place on screen (which action is affirmative shouldn't move as a form becomes valid)
  but the keyboard walks past it, since its click handler is inert anyway.
  Order is `NAV_TAB` → `LIST_TAB` → the form (10, 20, …) → `VALUE_TAB` + `i * ROW_TAB_STRIDE` for a
  growing list → `ACTION_TAB` for the footer, and that chain is asserted at **compile time** in
  `widgets.rs` (`const _: () = { … }`). It has already caught one regression: adding
  `ROW_TAB_STRIDE` cut the footer's headroom tenfold the day it landed.
  A **button's** focus signal is an outline, painted in `.focus`, *not* `.focus_visible` — floem
  gates `FocusVisible` on `app_state.keyboard_navigation`, which only its own `view_tab_navigation`
  ever sets, so a `focus_visible` rule on a ring member usually never fires at all. A **group**
  (below) deliberately shows nothing.
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
  focus it stands for.
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
- **`Editor::points_of_offset` returns *content* coords, not viewport-relative** (`.y` is `vline_y`,
  the absolute document y; the gutter view subtracts `viewport.y0` itself). Overlays pinned in
  `editor_area` must subtract `ed.viewport.get()` `x0`/`y0` to follow scroll — see `char_box`
  (bracket matching), `underline_seg_at`, `statement_line_boxes_at`, all tested against a scrolled
  viewport. **`editor_area` also doesn't clip**, so an overlay must bound itself: a box wider than
  the visible code column paints straight out of the editor and over the panel beside it, which is
  what `statement_line_boxes_at` clamps against `vp.width()` (a zero width means "not laid out yet",
  so it clamps nothing rather than blanking the overlay). The vertical half needs no clamp — floem
  won't place an offset outside its screen lines, and `editor_points` drops what it won't place.

## Popup menus (`menu_panel`)

Custom themed overlays, not Floem's native `Menu` (native renders OS-styled, clashes with the dark
theme). `menu_panel(entries: Vec<MenuEntry>, close)` takes `Action`/`Sub`/`Separator` entries and
renders the themed panel; the caller positions it absolutely. Used by the schema right-click menu
(`context_menu_overlay`).

- **Nested submenus**: a `Sub` entry hover-expands a child `menu_stack` anchored to the parent row's
  right edge (`inset_left_pct(100.0)` + `inset_top(-6.0)`). Recursive — each level owns its `open_sub`
  signal.
- **Hover intent (no timers)**: entering a leaf clears `open_sub`, entering a submenu row sets it;
  nothing closes on leave. The submenu is flush with the panel's right edge, so a diagonal move never
  crosses a gap — the close-on-diagonal problem is avoided structurally.
- **Dismissal**: the panel `on_event_stop`s its own pointer-downs, so the root "pointer-down anywhere
  closes" handler (in `workspace`) fires only for outside clicks. Escape and any action also call
  `close`. Submenus are view-tree descendants, so their clicks are absorbed by the root panel too.
- **Edge-flipping**: submenus flip left (`inset_right_pct(100)`) past the right edge and shift up past
  the bottom — from the parent row's window position (`on_move`/`on_resize`) + the live `window_size()`
  global (set from `workspace`'s root `on_resize`). `popup_menu_overlay` flips the whole panel the
  same way at the cursor. Size checks use conservative estimates (width ≈ 210, row ≈ 34) so there's no
  open-then-flip flicker.
- **Two menu channels**: the schema tree uses `ui.context_menu` (typed `CtxMenu`) +
  `context_menu_overlay`; everything else uses the generic `ui.popup_menu`
  (`RwSignal<Option<Vec<MenuEntry>>>`) + `popup_menu_overlay`. Both overlays live at the workspace
  root (window coords) and close on the root pointer-down.

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
  (drag-select, no capture); gutter click selects the row. Copy (Ctrl+C / toolbar) emits TSV; a lone
  cell copies its raw value.
- **Right-click menus** (generic `menu_panel` / `ui.popup_menu`): a header offers `Copy › CSV / JSON`
  of that column's values (`export_column_csv`/`_json`); a data cell offers `View`, `Edit` (editable
  cells only), `Copy`, `Set to NULL` (editable **and** nullable — stages `dirty` `None`), and
  `AI Summary` (reveals the AI panel, prompts with source table + column for context). The grid's app
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
  column maps to a base table), which is why both re-fetches go through the one `edit::refetch_key`:
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
  when schema isn't loaded). Flow: double-click an editable cell (or Enter) → inline editor; **Enter**
  stages into `gs.dirty` and paints the cell `grid_edit_staged()`; **Ctrl+Enter** / toolbar ✓ calls
  `commit_grid` → a `GridWrite { updates, inserts }` → the app's `commit_edits`. On success the app
  re-runs the query; on failure the error shows in the toolbar and green edits stay. No global "Edit"
  toggle. A read-only cell's double-click opens the value viewer.
- **Range selection is back, and it feeds the aggregates bar.** The state (`active`/`anchor`), the
  rect (`bounds`), the paint and `copy_selection` were always multi-cell aware; only the *input*
  had been gated off ("the grid has no multi-cell actions"). Shift+click, Shift+arrow and
  drag-select are live again — drag needs no pointer capture, just `gs.selecting` set on a cell's
  `PointerDown`, extended by each cell's `PointerEnter`, and cleared by the **body's** `PointerUp`
  (the release routinely lands outside the cell, past the last row, or outside the grid) **and by
  the double-click handler**, since floem's `DoubleClick` swallows the second `PointerUp` and the
  flag would otherwise stay armed with no button down. `Del` now drives the whole range to *one*
  state rather than flipping each row: on a mixed selection a per-row toggle both marks and
  unmarks, which reads as the key doing nothing.
  **Which column the arithmetic is about is the *anchor's*** — the one the selection started on, so
  dragging from `price` across to `name` still reports `price`. It reads `gs.anchor` rather than
  `bounds()`, which is a normalised rect and has forgotten which corner you began at. A selection
  covering *every* column is a row selection (gutter click, Ctrl+A, the Ctrl+G jump) whose anchor
  column is column 0 — usually an id, whose sum means nothing — so those get counts only; a
  single-column result is exempt, since there covering every column is covering the one you meant.
  `grid_selection_bar` renders at panel level (like the find bar, so it can sit at the panel's
  edge) while `grid_view` computes it, and it lifts itself above `grid_error_bar` when that one is
  up: they coincide exactly when a bulk delete fails, which is when both have something to say.
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
  to `sakila` still reports `sakila`.
- **Find (Ctrl+F) and Go to row (Ctrl+G)** are two popups sharing one anchor at the panel's
  top-right, and both are **split in the same way**: the bar renders at the RESULTS-*panel* level
  (`grid_find_bar`/`grid_goto_bar`, mounted in `results_section`) so it can sit at the panel's edge,
  while the work happens in `grid_view`, which is the only place that has the row data. Find is
  incremental on `find_query`; goto fires on a `goto_step` **nonce** the popup bumps on Enter,
  because a jump belongs to submit rather than to every keystroke. `grid_view` keeps at most one of
  the two open, as the editor does with its own pair. Go to row resolves through the pure
  `model::goto_row_index` — 1-based, in **display** coordinates (the gutter numbers what is on
  screen, so "row N" means the Nth row *as sorted*, and the total includes pending unsaved rows),
  **clamping** to the nearest end when the number is outside the grid: past the last row goes to the
  last, `0` goes to the first, and a number too wide for a `usize` clamps with every other overshoot
  rather than falling through to the not-a-number path. A row of 9s is how people ask for the bottom
  of a long result, and a silent no-op there can't be told apart from a broken feature, while
  overshooting is cheap to recover from — the gutter number and the row highlight say where you
  landed. `None` is left for the only two cases that can mean no row: an empty grid, and input that
  isn't a number. It then selects the whole row with the gutter click's own gesture (anchor column 0,
  active last column) and scrolls at **column 0**, so a jump doesn't also fling the viewport to the
  far right.
  **Closing either bar hands the keyboard back** (`focus_id.request_focus()`, on a true→false edge
  of the open flag). This is not optional and it is not the bar's job: Escape only flips the flag,
  floem then disposes the field's view and clears `app_state.focus` **silently**, and the grid was
  left focused on nothing — the next Ctrl+F reached nobody until the user clicked a cell. The bar
  can't do it either, being built a level up where `focus_id` doesn't exist, which is why the rule
  lives on the flag in `grid_view`. A new panel-level bar over the grid inherits this obligation.
- **Row actions: new / clone / delete.** Gated on a single writable table (`EditModel::insert_target()`;
  hidden for joins / read-only), committed in the shared `GridWrite` transaction (`commit_writes` runs
  **deletes → updates → inserts**, each exactly 1 row).
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
