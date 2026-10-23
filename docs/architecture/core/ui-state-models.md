# `schemaic-core` — small persisted / UI-state models

**Small persisted / UI-state models**, each a flat `Vec` keyed by `conn_id` and each pure +
tested (they share `history.rs`'s shape; a new one belongs here, not in the UI):
- `search_history.rs` — recent Find-Anywhere targets (`MAX_PER_CONN`, newest-first, deduped).
  `push` records only an *activated* result, not every keystroke, and the PG namespace is part
  of the dedup identity so same-named tables in two schemas don't collapse into one. So is
  `ObjectTag`, which is what makes an entry an enum/domain/sequence/function/procedure/event —
  or SQL Server's synonym, alias type or XML schema collection, persisted as `synonym`,
  `alias_type` and `xml_schema_collection` — rather than a table (its name rides in `table`, so
  every file written before objects were searchable still loads): a type and a table may share
  a name in one namespace and are different places to go back to. The three SQL Server tags
  went unwritten while the palette did not offer those objects — both `push` sites are its — and
  are written now that it does (under `object_editor.rs`). PostgreSQL's extension has a tag
  too, persisted as `extension`, so `ObjectTag::of` stays total over `ObjectKind`; nothing
  writes it today, the palette offering no extension (`is_editable_object` answers no for one).
  The tag is a **persisted** enum of its own rather than `ddl::ObjectKind` so a kind
  written by a newer build degrades instead of failing the file and losing every connection's
  history, the rule `SshAuth`/`Environment` follow; it resolves to no live kind, so the row is
  dropped from the recents list exactly as an entry for a since-renamed table is. `Unknown`
  **keeps the text it didn't recognise** and a hand-written `Serialize` writes it back
  verbatim — the obvious `#[serde(other)]` unit variant degrades the *file* but silently
  destroys the *value*, and since the app rewrites the whole of `search_history.json` on every
  change, merely running an older build once would rewrite a newer one's `"collation"` as the
  literal `"unknown"`.
  **`recent_visible` is what the palette actually reads, and it is the `db_hidden` rule's
  second surface finally honoured on both branches.** Find-Anywhere's *typed* branch filtered
  through `schema::db_visible`; its **empty-query** branch read `recent`, which asks only about
  `conn_id` — so a row for a database the user had put away with the SCHEMA eye stayed in the
  list, opened a tab on that database when activated, and disappeared the moment a single
  character was typed. The visibility test runs **before** the `MAX_PER_CONN` take and not
  after, or a hidden database's ten entries spend the whole window and leave the list empty;
  `a_hidden_databases_entries_do_not_spend_the_per_connection_window` is that half, and
  `a_hidden_databases_history_row_is_not_offered` the other. `recent` stays as it is — the
  unfiltered question is still the right one for a caller that has no hidden set to apply.
  **`clear_conn` is the deleted connection's half, and the prune was not the part that was
  missing** — this store's persistence is an effect, which can only save `Replacing`, so the
  prune's own save copied the *pre-prune* file to `search_history.json.bak` and left the
  deleted connection's table names, column names and searched text in the directory the
  Settings modal invites the user to open. `main.rs` holds an explicit `Saving`-taking saver
  beside that effect now; see the `create_effect` entry under *Floem 0.2 gotchas* for why the
  effect stays for an ordinary activation.
- `favorite.rs` — the `(conn_id, database)` star list. `toggle` appends newest-**last** on
  purpose: `rank` (0 = that connection's oldest) is what the schema tree sorts by, so order in
  the `Vec` *is* the sort key.
- `accepted.rs` — the completion items the user has taken, per connection (`AcceptedFile`,
  `accepted.json`), which `rank` reads as a bonus (under `rank.rs`). A rule is
  `(conn_id, text)` with a `count` and a `last` (Unix seconds), and the lower-cased text is the
  whole key: the list a caret offers has already decided what a word *is* there, and this only
  says how much the user has wanted it before. `record` bumps both; past `PER_CONN` (500) words
  on one connection it evicts **that connection's** longest-unused word, never another's
  (`a_full_connection_forgets_its_longest_unused_word_only`), so the file stays bounded however
  long it is used. `bonus(count, age)` is frequency in steps — 4 for one accept, 7 for two or
  three, 10 up to nine, 12 beyond — halved past a week and quartered past a month, plus 4 inside
  the hour or 2 inside the day, capped at `MAX_BONUS` (16). **The cap is the design, not a
  tidy-up**: it sits under a prefix match's +40 in `fuzzy_score`, so a word that matches what
  is typed better still wins over one merely chosen often; it is about the size of the
  in-statement `recency_bonus`'s 18. `bonuses` is the
  map `rank` takes for one connection, words earning nothing left out. **`BonusCache` keeps it
  between keystrokes**: building it walks every rule and clones each word, up to 500
  allocations on the undebounced recompute, while it changes only on an accept or a
  forgetting, or when a word's age crosses one of `bonus`'s steps (`STEPS`, which `bonus`
  itself reads). So it is rebuilt only when the connection's rules differ from the ones it was
  built from (compared, not cloned) or `now` has left the span in which no bonus can change
  (`the_cached_bonuses_are_always_the_fresh_ones`). `clear_conn` is the
  deleted connection's half, as on its siblings.
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
  **It returns `Option<Vec<DbHiddenRule>>`, and `None` means *not yet*.** The migration runs at
  most once and `save_ui` writes the flat field empty from then on, so a launch with no
  connection ids — `connections.json` failed to load, or the user deleted their last connection
  — produced no rules and lost the legacy list by the first save, turning every previously
  hidden database permanently visible with nothing said and no way to retry. `Some(vec![])` and
  `None` are therefore different answers: *there was nothing to migrate* against *there was
  nobody to migrate it for*. **The `None` is only half of what protects the setting**, and the
  other half is a caller that carries the unconsumed flat list back out to disk rather than
  writing the field empty: that half is `persist::migrate_legacy_once`, which hands the legacy
  list back whole for `save_ui` to write unchanged. The regression test that missed this
  asserted the opposite (`is_empty()`, which the bug satisfied), which is why the pinned one is
  named `no_connections_is_not_yet_rather_than_nothing_to_migrate` — and it still tests
  `migrate_flat` alone, so the composition it cannot see is pinned beside the function that owns
  it (`a_legacy_list_survives_a_migration_that_could_not_run`).
- `expanded.rs` — the `(conn_id, key)` set of open SCHEMA-tree nodes, modelled on `db_hidden.rs`
  line for line because it is **the third instance of that same mistake**. Every key it holds is
  name-only — `schema_tree::db_key` is `format!("db:{database}")`, and the same for
  `tbl:`/`col:`/`sch:`/`objgrp:` — so a flat `Vec<String>` meant expanding `sys` on one
  MySQL-family connection left it expanded on all of them, and since `information_schema`,
  `mysql`, `sys` and `performance_schema` exist on every such server the collision was
  guaranteed and landed on the largest databases on the box. The second connection's node
  rendered already open, built its whole table list, and — with the size column on — fired a
  `fetch_table_stats` against a database nobody had opened there; it survived restarts, because
  the flat list was persisted. `db_hidden`'s flat names and `schema::tab_target`'s global "last
  database" are the other two instances, both already written up as bugs in their own module
  docs, and all three are fixed the same way. `keys_for` resolves one connection's keys back to
  the `HashSet<String>` the tree reads, which is the question the tree is asking and what keeps
  every `contains` in `schema_tree` a one-argument call. `set_keys` replaces one connection's
  whole set and is the only mutator, because a toggle, *Collapse all* and collapse-this-database
  all rewrite the tree's set wholesale, so "here is this connection's set now" is the operation
  and a per-key insert/remove would be a second spelling of it; order within the connection
  being written **must not be relied on** (the runtime side is a `HashSet`, so the on-disk order
  is already whatever the set iterated), while every other connection's rules keep their
  relative order, which is what keeps a `ui_state.json` diff readable. `clear_conn` is how a
  deleted connection's expansions go out with it, which the flat set could not do at all: its
  keys stayed in `ui_state.json` forever and auto-opened same-named databases on connections
  created later. `migrate_flat` applies a legacy flat list to **every** connection and answers
  `None` — not `Some(vec![])` — when there are no connection ids, `db_hidden::migrate_flat`'s
  argument verbatim because it is the same migration: it runs at most once with the flat field
  written empty from then on, so a launch whose `connections.json` failed to load would
  otherwise collapse every tree permanently, with nothing said and no way to retry. The
  caller-side half of that — refusing the migration once and carrying the unconsumed list back
  out — is `persist::migrate_legacy_once`, shared with `db_hidden` because the two pairs differ
  in nothing but the rule type.
  **App side, `expanded` is the *active* connection's set and `expanded_rules` is the persisted
  truth.** A `create_effect` tracks `expanded` and writes it back through `set_keys`, reading
  `active_conn` **untracked** — that is the load-bearing half: tracking it would fire the effect
  on a connection *switch* and file the outgoing connection's set under the incoming one before
  anything replaced it. `switch_conn` sets `active_conn` first and *then* `expanded` from
  `keys_for`, which is what makes that untracked read land in the right place; the outgoing set
  is already stored, because the effect ran on every change that made it. Deleting a connection
  calls `expanded::clear_conn` beside the other per-connection stores.
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
  the file is why the app builds **one** `save_db_colors` for the pair (in `app/ui_stores.rs`,
  handed to both halves' `Stored`),
  since writing either half alone would drop the other, and why deleting a connection runs both
  `clear_conn` and
  `table_clear_conn`. A table's key is its **display name** (`schema::TableSource::display` —
  the bare name on MySQL/SQLite and inside PostgreSQL's `public`, `schema.table` outside it)
  rather than a separate `schema` field, because that spelling is already the identity an
  ER-diagram node id carries, so the diagram looks a colour up by node id with no reparsing.
  **`pick_color(presets, used, seed)` is a different question in the same file** — which
  identity colour a *new connection* gets — and it is here because it could not be tested where
  it was. `ui::pick_connection_color` read `SystemTime::now().subsec_nanos()` in the middle of
  its own body, so neither branch had a deterministic entry point and a decision function in the
  UI crate, with every one of its callers in `schemaic-app`, had no test anywhere in the
  workspace. That wrapper is now the two lines that read the clock and flatten
  `CONN_COLOR_PRESETS`. Getting this wrong is silent and durable: the identity colour is what
  the environment badge, the tab-strip rule and the editor frame use to tell production from
  local, so a regression handing out duplicates makes two connections look alike with nothing to
  notice it. One behaviour changed with the move — an unused preset is taken **in order** rather
  than by the seed, since the old spelling indexed the free pool by the clock too and two
  connections made in the same second could take the same colour with six others standing
  free; the seed still picks when every preset is taken. Matching is
  ASCII-case-insensitive, so a hand-typed `#e05252` counts against `#E05252`, and an empty
  preset list answers with the empty string rather than dividing by zero.
- `rank.rs` — the autocomplete **ranking policy**: which candidates a caret position offers and
  in what order. `rank(&SchemaIndex, &RankInput)` collects candidates into context tiers (dedup
  by text, first/lowest tier wins, and a candidate equal to the typed prefix is dropped), scores
  each by `fuzzy_score` plus `recency_bonus` plus `accepted_bonus`, and sorts by tier, then
  score, then length, capped at `MAX_ROWS`. `accepted_bonus` is what this connection has
  accepted before (`RankInput::accepted`, under `accepted.rs`) and applies to any kind — a
  keyword or a snippet as much as a name — where `recency_bonus` lifts identifiers only; it is
  asserted through `rank` itself (`a_word_accepted_before_ranks_above_its_equals`). `fuzzy_score` is a subsequence match that rewards a hit at a word's
  start — the name's, after `_`, or at a camelCase hump, so `oi` finds `OrderItem`'s words as
  it finds `order_item`'s (`a_camel_case_hump_scores_as_a_word_boundary`). It matches bytes
  while both sides are ASCII and characters once either is not, so `über` finds
  `Überweisung` and `é` never matches the halves of two other letters
  (`a_non_ascii_name_is_matched_by_its_characters`); the byte loop is kept for the common
  case because this runs per candidate per keystroke. **A table, column
  or database row inserts its name through `export::ident_if_needed`** where that differs from
  the bare name — `` `order` ``, `` `Unit Price` ``, PostgreSQL's `"OrderDate"` — while the row
  shows, and was matched by, the bare one. That is `accepted_insert`'s answer, asked at accept
  for the one row taken rather than while ranking, and never over a row's own `insert` (an FK
  join target's `table_sql` is quoted already). So the FK `ON` predicate row is built by
  `rank::join_condition_row`, which carries the predicate as its own `insert`: built in the UI
  with none, it was a column row whose text failed `ident_if_needed`, and went in quoted whole
  as one name (`the_join_condition_row_inserts_its_predicate_unquoted`). **Inside a quoted name the name goes in bare**:
  completion runs there on purpose (`intel::caret_in_literal_or_comment` lets a quoted name
  through), and the quote before the word is the user's, so the UI asks
  `intel::caret_in_quoted_name` — the same probe, answering the other kind of span — and passes
  it in (`a_name_that_needs_quoting_inserts_quoted`). For a **table** row that check comes
  *ahead of* the row's own `insert`, which is otherwise never overridden: a qualified insert —
  the tab's list's `sales.ledger`, the widened list's references, which had the flaw first —
  landed after the user's opening quote as one identifier with a dot in it
  (`a_qualified_table_row_inserts_its_bare_name_inside_a_quote`). **`rank_full` is `rank` plus the one fact the rows cannot carry**:
  `Ranked::completes_a_word`, whether the word typed already spells a candidate in full. The
  exact match is left out of the rows, and that was also what hid it — after `FROM` the list
  held `FROM_BASE64()` and `FROM_DAYS()`, and Enter, meant as a newline, accepted the first (the
  key is under `editor_pane.rs`). So the exact match is still put into `seen` before it is
  turned away, and the flag is `seen.contains` of the lower-cased prefix, never `true` for an
  empty one; `rank` is `rank_full(..).items`, so no other caller moved
  (`a_word_typed_in_full_is_reported_though_not_listed`). **The order inside
  `!seen.insert(tl) || exact` is load-bearing**: written `exact || !seen.insert(tl)`, which
  reads the same, the short circuit skips the insert for exactly the candidate the flag is
  about, and it is never set. **`RankInput::dialect` decides which builtin catalog the function tier is
  drawn from, and it is not a nicety**: the `ClauseCtx::Column` arm used to walk
  `intel::FUNCTIONS` whatever the engine, so a PostgreSQL tab was offered `IFNULL` and
  `GROUP_CONCAT` — names that engine does not have — and never `btrim`, which it does, while a
  SQLite tab got MySQL's list just as silently. It is the typo checker's original PostgreSQL bug
  in the surface that *inserts* the word rather than the one that underlines it, so the arm asks
  `intel::offered_builtins(input.dialect, input.flavour)` — the one engine→catalog map, read
  through `intel::CatalogIndex` and `pub(crate)` for this caller — and reads its `None` as an empty
  slice, leaving a fifth engine offered nothing rather than inheriting whichever list is
  nearest. `each_dialect_is_offered_its_own_builtins` asserts
  that through `rank` and not over the catalogs, because the catalogs were already right and the
  composition was what was broken; `ranked_on` is the existing `ranked` helper with the engine
  named, and `ranked` itself still passes `SqlDialect::MySql`, so every older test means what it
  did. **The size of PostgreSQL's catalog reaching the popup was the half that fix left open,
  and the arm now filters through `intel::is_offered_builtin`** — `true` on MySQL, SQLite and
  SQL Server, whose catalogs are already the user-facing list, and `pg_builtins::PG_SUGGESTED` membership on
  PostgreSQL, so the tab is offered 863 names rather than 2,706 while the checker goes on
  trusting all of them. **That filter is precomputed now and the arm reads its answer**: it is a
  binary search on PostgreSQL, asked per candidate, so it ran 2,706 times per keystroke to
  arrive at the same 863 names every time. `offered_builtins` holds them, beside the typo
  checker's own rearrangement of the same data.
  `postgres_offers_the_subset_and_the_checker_still_sees_the_rest` asserts
  **both** halves of that in one test, because either alone is satisfiable by a bug: a plumbing
  name is required to be *in* `PG_FUNCTIONS` and *absent* from the ranked list, and `coalesce`
  and `greatest` — grammar-only forms no `pg_proc` query can return — are required to survive.
  **`RankInput::flavour` is the same question one level finer, and the dialect could not ask
  it**: `SqlDialect` has no MariaDB arm, so `intel::FUNCTIONS` served both servers and a MySQL 8
  tab was offered the forty-nine names MariaDB has and it has not — `NVL`, `TO_CHAR`, the eight
  `COLUMN_*` — while a MariaDB tab got five of MySQL's. It is a `ServerFlavour`, not a fourth
  dialect, because everything else about the two is the same dialect; `Unknown` offers
  everything, which is what this did before the split and is the deliberate choice
  `intel::is_offered_builtin` argues for. No new signal carries it: the caller reads it off the
  loaded `DbSchema` for the tab's active database, where `collect_schema` stamped it.
  **A builtin the engine calls bare is inserted bare.** `accept_completion` writes a function
  row as `name()` with the caret between the parentheses, so `CURRENT_TIMESTAMP` went in as
  `CURRENT_TIMESTAMP()` — a syntax error on PostgreSQL, SQL Server and SQLite. The `Column` arm
  sets such a row's `Suggestion::insert` to the name alone wherever `intel::called_bare` — the
  `niladic_builtins` set the column resolver already exempts — says the engine calls it so,
  and an `insert` is spliced verbatim; the row stays a `Function` for its glyph and colour
  (`a_builtin_called_bare_inserts_without_parentheses`, every engine, with `coalesce` still
  `None` beside it). **The `cands.len() > before` guard is load-bearing**: `add` returns
  without pushing for a name the prefix filter, the dedup or the exact match turns away, and
  without the guard `cands.last_mut()` would stamp the name onto whatever was pushed last.
  **The database's own routines are offered where each can be called**, from
  `SchemaIndex::routines` (the in-scope database's `DbSchema::routines`, as `IndexedRoutine`).
  A function joins the builtins wherever an expression goes (`Column`, `Value`), ahead of
  them so one named like a builtin claims its row, its detail `(arguments) → returns`; a
  trigger function, which no expression can call, is left out. A procedure is offered at
  `ClauseCtx::Routine` — the name a statement-opening `CALL` (MySQL, PostgreSQL) or
  `EXEC`/`EXECUTE` (SQL Server) runs, and nothing past it (`intel::routine_slot`) — where the
  list opens on an empty word as it does after `FROM`. It is a `Function` row, so it goes in
  as `name()`, except where `intel::procedure_args_in_parens` says the call passes its
  arguments bare, T-SQL's `EXEC p @a = 1`, which inserts the name alone
  (`the_databases_routines_are_offered_where_each_can_be_called`,
  `the_name_a_call_or_exec_runs_is_a_routine_slot`).
  **A routine goes in by the name that reaches it** (`rank::routine_sql`): `IndexedRoutine`
  carries its schema, and the row inserts `schema.name` where no bare name reaches that schema
  (`filter::bare_name_reaches`, as for a table) and, for a function, wherever
  `intel::user_function_call_needs_schema` says the engine calls one only by its two-part name
  — SQL Server, whose bare `f()` is Msg 195 even in `dbo`. Each part goes through
  `ident_if_needed`, so PostgreSQL's `"calcTotal"` keeps its quotes. Such a row's `insert`
  ends in the call's `()`, and `rank::override_splice` puts the caret between them at accept,
  or drops them where the call's parens already follow, as a plain function row's `name()`
  does (`a_routine_inserts_the_name_that_reaches_it`,
  `a_routine_override_places_the_caret_in_its_parens`).
  **The keyword lists are filtered per engine** in every arm that offers them, through a local
  `keywords` closure over `intel::keyword_offered`: `SQL_KEYWORDS` and `STMT_KEYWORDS` are
  shared, and a word the engine has no use for is a row that writes an error — `LIMIT` on SQL
  Server, `AUTO_INCREMENT` anywhere but MySQL, `DESCRIBE` off it
  (`a_keyword_is_offered_only_on_an_engine_that_takes_it`). What each engine lacks is
  `intel`'s `keywords_not_taken`, an exhaustive `match`, so a fifth engine has to answer
  rather than inherit MySQL's empty list. **Each engine's own keywords follow the shared
  ones**: a `clause_keywords` closure chains `intel::engine_keywords(dialect, flavour)` onto
  the filtered `SQL_KEYWORDS` at all four sites that offer it, in the same tier — 3 in
  `Column`, 1 in `Value` (0 where the typed word begins it, the builtins sharing tier 0), 2 in
  `Other`, and 1 at `Start`, so `MERGE` begins a statement on PostgreSQL and SQL Server
  (`each_engine_is_offered_its_own_keywords`). **In `Column`, a keyword the typed
  word begins is promoted to the builtins' tier (2)** (`begins_with`), where the prefix bonus
  in `fuzzy_score` lifts it past every builtin that only fuzzy-matches. Left at 3 it could be
  crowded out entirely: the sort is by tier first, and `parti` is a fuzzy match for enough of
  PostgreSQL's 863 offered builtins to fill `MAX_ROWS` before `PARTITION` was reached
  (`a_keyword_the_word_begins_outranks_a_builtin_it_only_fuzzy_matches`). A keyword matched
  only fuzzily stays at 3, and so does every keyword with nothing typed, where every word
  "begins" with the empty prefix and the promotion would put every keyword — `SQL_KEYWORDS`
  alone holds 57 — among the functions.
  **A keyword row is in the typed case**: `add` writes it through `intel::in_typed_case`, the
  inline predictions' rule — all lower typed, lower offered; anything else, and nothing typed,
  the conventional upper — so `sel` accepts `select` (`a_keyword_is_offered_in_the_typed_case`).
  A name keeps its own spelling whatever the prefix.
  **`ClauseCtx::Database` is `USE |`**, which `intel::clause_context` answers for a statement
  whose only word so far is `USE` (`use_names_a_database`): its arm offers every database but
  the tab's own, with no prefix needed, and nothing else — the mixed list it fell into before
  offered tables and columns where only a database can go (`use_offers_the_other_databases`).
  **`ClauseCtx::Type` offers `intel::cast_target_types` and nothing else** — no columns, no
  grammar continuations — as keyword rows, so in the typed case
  (`a_cast_target_offers_the_engines_types`). **MySQL's `CAST` takes a list of its own, not
  its column types**, and the two servers' lists differ: measured with `SELECT CAST('1' AS …)`,
  MySQL 8.4.11 refuses `INT`, `INTEGER` and `VARCHAR` and takes `JSON`, `YEAR` and `REAL`;
  MariaDB 10.11.14 takes `INT`/`INTEGER` and refuses those three; eleven are common to both. A
  MySQL-family tab whose flavour is not known yet is offered both, as `engine_keywords` does.
  PostgreSQL, SQLite and SQL Server take any type name, so theirs are `ddl::common_types`'
  base names, upper-cased for `in_typed_case`
  (`each_engine_is_offered_its_own_cast_targets`).
  **The statement's own names arrive as `RankInput::local`** (`LocalNames`), read by the
  caller only where they are offered: `intel::ctes_at` at a table slot, whose CTEs lead the
  `Table` arm (tier 0, detail `CTE`, ahead of the FK targets so one shadowing a table claims its
  row), and `intel::output_aliases_at` at a column slot, whose aliases lead the `Column` arm
  (tier 0, the alias glyph). Each is an `intel::LocalName` and **inserts as the statement
  spelled it** (`written`), never through `accepted_insert`'s quoting: PostgreSQL's unquoted
  `AS Total` names `total`, and quoting it by the engine's rule would name something else
  (`the_statements_own_ctes_and_aliases_are_offered`). `output_aliases_at` answers only in the
  caret's own query's `ORDER BY`, or its `HAVING` where `having_sees_output_aliases` —
  measured: MariaDB 10.11.14 and SQLite 3.45.1 resolve `HAVING x` against the select list,
  PostgreSQL 16.15 and SQL Server 2022 refuse it, and all four take an `ORDER BY` alias —
  but PostgreSQL and SQL Server only as a whole sort item, so there the aliases are offered
  only right after the `BY` or a comma, not inside `1 + …` or a `CASE`
  (`order_by_expression_sees_output_aliases`, measured the same way with `ORDER BY 1 +
  doubled`;
  `an_order_by_expression_sees_aliases_only_where_the_engine_resolves_them`). It
  parses the query up to the keyword from `local_scope_start`, as `group_by_items` does, so a
  subquery's aliases stay its own and a window's `OVER (ORDER BY` reads nothing; after a set
  operation an `ORDER BY` takes the first arm's names and a `HAVING` its own arm's
  (`order_by_and_having_see_the_select_lists_aliases`). `ctes_at` is `cte_names`' walk
  (`cte_name_tokens`) with each name's spelling kept.
  **A second Ctrl+Space widens the table list** (`RankInput::wide`): every table of every
  contributing database and schema (`SchemaIndex::all_tables`), under the tab's own list, each
  **fully qualified** — `filter::table_name_from` gives the reference that reaches it from the
  tab's database: MySQL's `db.t`, SQL Server's `schema.t` and `db.schema.t` from another
  database, PostgreSQL's `schema.t` and nothing from another database, which its connection
  cannot reach (`the_widened_list_offers_every_table_fully_qualified`,
  `a_table_is_named_from_the_sessions_database`). The row shows the reference with its quotes
  off and is matched by it. **A view's row wears the view glyph** (`SuggestGlyph::View`, the
  schema tree's `TABLE_CELLS_MERGE`), from `SchemaIndex::views`, at a table slot and after its
  database's qualifier (`a_view_is_told_from_a_table`). Where the row names a schema it is told
  by `SchemaIndex::views_by_schema` instead: after a `schema.` qualifier, and for a bare row
  by the namespace the bare name reaches (the first `filter::bare_name_reaches` lets through,
  else the first, as `qualify_unreached` writes it) — keyed by database alone, `sales.` never
  showed the glyph, and a view `archive.orders` marked the bare `orders` row that reaches the
  table `public.orders` (`a_view_is_told_by_its_schema`). In the UI `Completion::wide` is set by a Ctrl+Space over an open
  list, kept while it stays open, and cleared when a list next opens by typing.
  **The tab's own list qualifies a table no bare name reaches.** It offered every in-scope
  table under its bare name, so PostgreSQL's `sales.ledger` went in as `ledger` — which reads
  whatever `search_path` finds first, or nothing. `SchemaIndex::namespaces` maps each `tables`
  name to the namespace of every in-scope table carrying it, and `qualify_unreached` (beside
  `mark_view`, and guarded by `cands.len() > before` for the same reason) runs after each table
  row in both `ClauseCtx::Table` and `ClauseCtx::Other`: when **every** namespace of the name
  fails `filter::bare_name_reaches`, the row's `insert` becomes
  `filter::qualified_table_name` with the first of them, while the row still reads as the bare
  name. SQL Server takes the same path with `dbo` as its default. A name with one table in the
  default namespace stays bare — the rows are deduped by text, so that row stands for both, and
  the bare name reaches one of them (`a_table_outside_the_default_namespace_inserts_qualified`:
  `sales.ledger` qualified, `public.orders` bare, `audit` in `sales` and `public` bare). Two
  same-named tables both outside the default get the *first* namespace the index met; the
  widened list is what names the other.
  **Straight after a sigil the list is the document's own names and nothing else**:
  `ClauseCtx::Variable` after one `@` where the engine has `@` variables (`has_at_variables`:
  SQL Server's declared ones, MySQL's user variables — not `@@`, a system function's) offers
  `intel::variables_at`, and `ClauseCtx::TempTable` after a T-SQL `#` or `##` offers
  `intel::temp_tables_at` — each read beyond the statement, since a variable or temporary
  table outlives it, through the one boundary lexer (`sigiled_names`), the word being typed
  left out. A temporary table is read from the whole document; a variable from the whole
  document on MySQL, whose user variables live for the session, and from the caret's own
  batch on SQL Server, where a `DECLARE` does not outlive its `GO` (`sql::batch_range`, which
  takes a `GO` at a line's start in code only — `a_t_sql_variable_is_offered_only_in_its_batch`).
  A temporary table is offered only with the sigil typed (`#` local, `##` global) and named
  without it, since the replaced word starts after it
  (`the_documents_variables_and_temp_tables_are_read`,
  `a_sigil_before_the_word_makes_a_variable_or_temp_table_slot`,
  `a_sigil_offers_the_documents_variables_or_temp_tables`). **Its columns** come with it
  where the statement names one: `intel::temp_table_relations` reads every `CREATE TABLE
  #name (…)` and `SELECT … INTO #name` in the document, each statement parsed alone, into
  `LocalNames::relations` under the name with its sigil, which is how the scope and
  `#work.` name it (`a_temp_tables_columns_are_read_from_where_it_is_made`). A table the
  script drops and makes again has its last definition before the caret, the one the caret's
  statement meets, and not its first
  (`a_remade_temp_table_has_its_latest_columns_before_the_caret`). The caller
  reads them only when the scope names a `#` table, since the reading parses every
  statement that makes one — and once that was so it still parsed them on every keystroke
  and every caret move in such a slot, though only an edit can change the answer. So
  `temp_table_relations` is now two halves composed: `temp_table_definitions(sql, ranges,
  dialect)`, the parse, which knows no caret and returns every `TempTableDef` (name, columns,
  the byte its statement starts at) with each remaking kept; and `temp_table_columns_at(defs,
  caret)`, the cheap fold that picks a remade table's definition for the caret. The editor
  keeps the first in an `intel::TempTableCache`, keyed like `stmt_index` on the text revision,
  the dialect and the text's length, and cuts the statements from the pane's `StatementIndex`
  rather than a fresh walk — so only an edit pays the parse
  (`the_temp_table_cache_reads_a_text_once`,
  `temp_tables_from_the_index_ranges_answer_what_the_walk_does`).
  **A CTE's and a derived table's columns are offered as a table's are**, from
  `LocalNames::relations`: `cols_of` asks them first for a bare name, since a CTE shadows a
  table it is named after, and the schema otherwise. The caller reads them with
  `intel::local_relations` at a column or qualified slot, and adds each derived table's alias
  to the scope, which holds no reference for one — the tables inside are its own query's,
  in scope only with the caret inside it. `local_relations` finds them by the lexer — `WITH name [(cols)] AS [[NOT]
  MATERIALIZED] (…)`, and a `(SELECT …)` after `FROM`, `JOIN`, `LATERAL` or a list's comma
  followed by its alias — because the statement being written does not parse whole; each
  **body** is parsed alone, and the one still being typed gives nothing. Its columns are a
  given column list, else the leftmost select list's names (a column's own, an alias's); an
  unnamed expression and a `*` name nothing knowable and are left out
  (`a_ctes_columns_are_its_select_lists_names`,
  `a_derived_tables_columns_are_its_select_lists_names`,
  `a_ctes_and_a_derived_tables_columns_are_offered`).
  **A column two tables in scope share is inserted qualified** — `c.id`, by the alias of the
  table whose row it is (the last table claims a shared name), else that table's own name —
  because bare, the server refuses it as ambiguous. Not with one table in scope, nor where
  the slot takes a bare name only: a join's `USING (…)` list, which `intel::in_using_list`
  reads as the innermost paren open at the word being a `USING (`'s and the caller passes as
  `LocalNames::bare_columns`. **A join's `USING`, read back to its `JOIN`**
  (`using_follows_a_join`, over the table, its alias and a closed derived table): a `MERGE`'s
  source and PostgreSQL's `DELETE … USING (SELECT …)` put a whole derived table in that paren,
  and every column slot in it went in bare — `id` that `deliveries` and `items` share, which
  SQL Server called ambiguous (S8.4-L1-02, `a_merge_or_delete_source_is_no_using_list`, red
  first). Inside a quoted name `accepted_insert` gives a column its bare
  name, as it gives a table (`a_column_two_tables_share_is_inserted_qualified`,
  `a_using_list_is_told_from_other_parens`).
  **A name the statement wrote is not quoted as if the catalogue had spelled it.** A local
  relation's column, an in-scope alias row and the shared column's qualifier go in through
  `intel::written_name_sql`, which on a folding engine writes a name that is plain once folded
  as written: PostgreSQL read a bare `Total` or `O` as `total`/`o`, so `accepted_insert`'s
  `"Total"` and `"O".id` named nothing. An unaliased table's name is written as the catalogue
  spells it, the table the server resolved however the statement cased it. The FK predicate
  writes its qualifiers the same way (`intel::build_predicate`, from the `FkEdge`'s own
  spellings). Its quotes are off by the time a name reaches ranking, so a quoted `"O"` alias is
  the one spelling this cannot tell from a bare `O`, and takes the bare one's reading
  (`a_name_the_statement_wrote_bare_is_not_re_quoted`,
  `a_bare_upper_case_qualifier_is_not_quoted_on_a_folding_engine`).
  **Inside a string compared with an enum or set column, the list is that column's members**
  (`enum_value_suggestions`, which `rank` never reaches — its caller is the literal gate's
  exception, under `completion.rs`). The column is resolved in the statement's scope through the slot's
  qualifier — matched against a table's alias, or its name where it has none — else the first
  scope table that has it, through `SchemaIndex::columns_of` — the
  `columns_by_db`/`columns_by_schema`/`columns` lookup `rank`'s `cols_of` asks too. Members come from
  `celledit::editor_for_type`, the grid cell editor's reading of a MySQL `enum(…)`/`set(…)`,
  or, for a PostgreSQL column whose type carries only the enum's name, from
  `SchemaIndex::enum_types` — labels by lowercase `name` and `schema.name`, the bare key taking
  the first enum the index met. They keep declaration order, which for an enum is its meaning,
  and are narrowed to a case-insensitive prefix of what is typed. Each row is a `Column` with
  the type as its detail, `replace`s the literal's contents and inserts the member through
  `export::sql_literal` with its quotes off — so `won't` arrives as `won''t` — plus the
  closing `'` the literal lacked (`a_column_s_enum_members_are_offered_in_its_string`,
  `an_enum_member_is_written_as_a_literal_and_closes_its_quote`).
  **`add` asks `worth_offering` before it builds anything, which is `add_col`'s shape adopted
  for `add_col`'s reason.** A PostgreSQL tab offers 863 builtins where a MySQL 8 one offers 341
  (`FUNCTIONS`' 463 less `MARIADB_ONLY`, which contains every `DEPRECATED_ALIASES` name), and
  every one of them was paying a `to_ascii_lowercase`, a `HashSet<String>` insert and an eagerly
  allocated `String` for its signature before the ranking below could drop it — on a path that
  is deliberately undebounced (`exec_after(Duration::ZERO)`, one tick per keystroke). `detail`
  is a `&str` now, so that allocation happens after the guards rather than before them. **The
  pre-filter sits above the dedup**, on the argument `add_col` sets out: `worth_offering`
  depends on nothing but the candidate's own text and the prefix, so a candidate it refuses
  cannot be blocking one it would admit — a later duplicate has the same text and fails the same
  test. `the_pre_filter_admits_exactly_what_the_ranking_keeps` is what holds that, and it covers
  this call site now as well as `add_col`'s.
  Measured per keystroke in a release build, `ClauseCtx::Column` with one table in scope: a
  PostgreSQL prefix of `"cu"` went from 511.2 µs to 40–45 µs and `"sub"` from 493.7 µs to
  20–23 µs; MySQL's from 86.6 µs to 10–12 µs and from 72.6 µs to 6 µs. **An empty prefix is the
  case this does not help, and the figures are reported rather than flattered**: nothing is
  pre-filtered when every candidate matches, so that path gains only the offered subset and the
  late allocation — PostgreSQL 590.1 µs to 318–407 µs, while MySQL's 146.7 µs against 171–226 µs
  moved within build-to-build variation and is not attributable to this change, which was
  checked by short-circuiting the pre-filter on an empty prefix and finding nothing changed.
  What that path costs is scoring and sorting the whole pool, which this change does not touch.
  `SchemaIndex`/`ColMeta`/`Suggestion`/`SuggestKind`/`KeyKind` are the
  vocabulary; `worth_offering` and `database_suggestion_visible` are the leaf rules `rank`
  itself composes. **`statement_identifiers` and `snippet_abbrev_rows` are not** — they are the
  *caller*'s leaf rules, called directly by `completion::recompute_completions` and handed back
  in as `RankInput::used` and `RankInput::snippets`. They live in this module for the same
  reason the ranking does and not because `rank` reaches them: they are decisions, and in
  `completion.rs` nothing could test them. **It is here because it could not be
  tested where it was** — all of it was the middle of `completion::recompute_completions`, a
  519-line function taking `&Editor`, so the tier rules, the FK demotion, the last-table-in-scope
  bias, qualifier resolution and the comparator were unreachable from any test, and two
  performance findings had to re-implement parts of it in a replica to measure them. What stayed
  in `completion.rs` is what genuinely needs a mounted editor: the caret offset, the document
  text, the popup anchor and the signal writes, plus the gathering of the schema index, the
  statement scope, the snippet rows, the FK join targets and the star expansion — each of which
  reads a signal. **A `Suggestion` carries a `SuggestGlyph` rather than an icon**: the *reason*
  for its leading glyph (a column's type family, an FK target, an in-scope alias), which
  `completion::glyph_icon` turns into an SVG, so no `&'static str` icon body reaches core.
  **And it carries a `RowWords` where its words are Schemaic's own** rather than a name, a type
  or a signature: `Suggestion::words` (and `Cand`'s) is `ForeignKey` on the FK join target and
  `join_condition_row` (detail *foreign key*), `ExpandStar { columns }` on the star expansion
  (text *expand \**, detail the column count), and `None` on every other row — the popup showed
  those words in English whatever the interface's language. The producers still write the
  English into `text` and `detail`, through `RowWords::text` and `detail`, so `text` is what
  `fuzzy_score` matches as before and no English test moved; the popup says them in the
  interface's language (`shown_words`, under `completion.rs`). A CTE row's *CTE* is not among
  them: it is an SQL term.
  **The comparator is asserted by *calling* `rank`, which is what being callable was for.**
  `completion.rs` held a test named `the_callers_comparator_preselects_the_prefix_match` whose
  own doc claimed it covered the composition — and it built its rows from `fuzzy_score` and
  sorted them with a **copy** of the comparator written inline, a copy missing the tier and the
  `recency_bonus` term. `recompute_completions` sets `sel = 0` and `accept_completion` splices
  `items[sel]`, so this sort *is* what Enter inserts; flipping the real one to score-*ascending*,
  so the popup preselects the worst match, left the whole workspace green, because the replica
  sorted its own vector. It is deleted, and the two that replace it are here:
  `the_comparator_preselects_the_prefix_match_and_buries_the_interior_one` — prefix match first,
  a non-match absent from the list entirely, and the interior match last of the three, compared
  against its siblings rather than against the whole list, which also holds keywords and
  functions — and `a_recently_used_name_is_lifted_past_a_better_scoring_one`, which is the term
  the replica dropped. Both fail against that flip. **And the star-expansion arm had never been
  called at all**: every one of the six construction sites in the workspace passed `star: None`,
  so the row that replaces the `*` was asserted nowhere — not its wording, not its `replace`
  range, not its `insert` — and restoring a hard-coded `"{ncols} columns"` over the singular
  `1 column` passed the suite.
  `the_star_expansion_row_says_its_column_count_and_replaces_the_star` is that caller.
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
  `rebind_needed(tab, active_conn, picked, known)` is the **database selector's** half: does
  picking a row in it actually move the tab? Three ways the answer is no and the app asked only
  the first — the name is not one the selector lists, or the tab is already on it *on the
  connection the pick is made against*. The selector invites the second, because it renders the
  current row accented rather than disabled, unlike every other already-in-that-state entry in
  the app — and re-picking it is not free: the rebind cancels the tab's in-flight query (a
  30-second `SELECT` goes `Cancelled` with nothing saying why and the database unchanged), raises
  the Commit/Rollback/Cancel prompt on a Manual tab holding an uncommitted `INSERT` — where
  answering Rollback discards a transaction for a move that is not one — and either answer then
  drops and re-opens the pinned session. The connection is half the question: a tab keeps the
  connection it was opened on and the rebind writes `conn_id` as well as `database`, so the same
  name on another connection *is* a move. `set_tx_mode`, the sibling action that also settles a
  transaction and re-pins a session, opens with exactly this refusal;
  `app_tests::set_active_db_refuses_a_rebind_that_is_not_one` is the half the core tests cannot
  see, which is whether the caller asks.
  `scoped_database(tab, active_conn, fallback)` answers the other question about the focused
  tab: the database a request about "the current tab" should run against — the tab's own, but
  **only when that tab is on the active connection**, otherwise the caller's already-scoped
  fallback. Switching connections doesn't move the focused tab and a tab keeps the connection
  it was opened on, so the focused tab routinely names a database that lives somewhere else;
  handing that name to the active connection's `Db` is how the MCP endpoint came to ask MariaDB
  for `chinook`. It is here rather than in each of its four callers — the AI's turn context
  (`app/ai.rs`), the terminal's DB-CLI button (`app/main.rs`), the AI proposal card and the chat
  code block's **Insert**/**Run** (both `ui/ai_panel.rs`) — because the last two are not
  harmless. The card's answer goes to
  `ddl_preview::preview_proposal`, which pairs it with `edit_ctx`'s *active* connection and
  stamps that `conn_id` into the plan `run_ddl` executes, so getting it wrong runs an `ALTER`
  on prod from a proposal written about dev. With no database to name, the card refuses and
  says to switch to that tab first. Insert had the same seam one step earlier: `open_query`
  builds its tab from `default_tab_target()`, whose `conn_id` is the **active** connection, and
  it took the focused tab's database name unconditionally — and a dev/prod pair almost always
  carries the same database name on both, so a `DELETE … WHERE status = 'draft'` written about
  dev opened on prod — and `run_verdict` passes a `DELETE` that has a `WHERE`, so nothing on
  screen named the server it went to. It refuses rather than re-targets for a reason of its own:
  `place_tab` does not switch connections, so a tab opened on the *focused* tab's connection
  would not appear in the strip at all, which reads as nothing having happened. And
  `CodeActions::insert` answers `bool` **so that *Run* knows not to run afterwards**: `run`
  targets the active tab, so running after a refused insert would execute the block in whatever
  tab is in front — the connection the insert had just declined to open on. That copy had the rule
  spelled out inline, expression for expression and untested, because `schemaic-ui` cannot
  depend on `schemaic-app` — which made it a misplaced function rather than an unavoidable
  duplicate.
  **`scoped_database` is a reading of `tab_scope` now, because one caller has to tell its two
  `None`s apart and could not.** `TabScope::{Bound, NoDatabase, OtherConnection}` keeps "the
  focused tab is on another connection (or there is no focused tab)" separate from "it is on this
  one and has no database bound", which `scoped_database` is right to fold together — both mean
  *do not use the focused tab's database* — and wrong to fold for a caller whose next act is to
  explain the refusal. The chat code block's Insert/Run raised *"This chat is about a tab on a
  different connection… switch to that tab"* for the second case, which happens when every
  database is hidden or the schema had not finished loading when the tab was opened: the first
  sentence false, and the remedy naming the tab the user was already on. **The chat block's
  *Propose* is the second caller and it kept the old sentence** — the one its own comment calls
  "the one caller that can destroy something", twenty lines below the arm that had just been
  corrected — so both now match on the enum; the other two keep asking `scoped_database`, and
  `scoped_database_still_answers_exactly_what_the_scope_says` walks every state so the two cannot
  drift.
  **The reopen ring's two ends are both here, and they are not the same predicate.**
  `worth_remembering(query, has_source, has_name, has_path)` is the write end — does a closing
  tab carry anything to restore — and `has_reopenable(closed, conn)` the read end, which the
  menu entry dims on, asked *per connection* because the ring spans them and reopening does not
  (a non-empty ring holding only another connection's tabs offered a click that did nothing).
  `worth_remembering` is deliberately **not** `!is_blank_slate`, although both look like
  "is this tab empty". `is_blank_slate` asks whether a freshly-built tab may replace the active
  one *in place*, and weighs `pinned` and `results_untouched` — neither of which says anything
  about what is worth keeping: a pinned tab never reaches the closing path at all, and a merely
  *scrolled* result is not work, since a reopened tab re-runs its query. `worth_remembering` in
  turn weighs `source` and `name`, which the reuse question does not ask about — and `source` is
  the one that matters most, because a table tab is the most-reopened kind there is and carries
  no query text whatsoever. Spelled as one negation of the other, the pair would answer for six
  terms between them and each would carry two it does not mean.
  They agree on the term both do hold: a tab bound to a `.sql` file is neither reusable nor
  forgettable even when the file is empty, because the binding to the path is the thing at stake.
  `closed_tab_choice(name, path, label, query)` is how one ring entry reads in the palette's
  *Reopen Closed Tab*: the title the tab had — its name, else its file's (`sqlfile::tab_title`),
  else *Query N*, or a bare *Query* for label `0` — then ` — ` and the start of its text on one
  line (`history::preview`), so two closed *Query 3*s from different sessions can be told
  apart (`a_closed_tab_reads_as_its_title_then_its_text`). The text is cut at 80 characters with
  a `…`, counted in `char`s and sliced on a boundary (`a_long_closed_tab_is_cut_on_a_character`).
- `palette.rs` — parses the command palette's `>` command mode into
  `Parsed::{Search,Filter,Command{name,arg}}`. The hard part is when typing stops filtering the
  command list and becomes an argument: longest-word-prefix match against the caller's
  argument-command names, under an invariant the caller must uphold — no argument-command name
  may be a word-prefix of another (`indent style`/`indent width`, never a bare `indent`), across
  every name passed: a command arrives under its translated label *and* its English name, and
  `Command { name }` carries whichever spelling matched. The match is a character at a time,
  case-insensitive, and cut at the **input's** own byte offset (`after_word_prefix`). It used to
  compare `to_ascii_lowercase`d input with the name and cut at the name's byte length, so a
  Tab-completed German `>Größe Ändern` never matched — `Ä` is not ASCII — and a full
  `to_lowercase` would not have fixed it, since lowercasing can change a string's byte length
  and its offsets would no longer index the input
  (`a_name_beyond_ascii_matches_case_insensitively`).
- `resource.rs` — the status bar's CPU/RAM model. `ResourceSample::new` divides `sysinfo`'s
  per-process CPU% (single-core-relative, so it exceeds 100 on a multi-core box) across the
  logical core count to give a whole-machine 0..=100. Sampling itself stays at the app boundary,
  and **runs only while Settings › Appearance's *Show CPU and RAM in the status bar* is on**
  (`UiState::show_resources`, `serde`-defaulted on, so a file from before it reads on —
  `the_resource_readout_defaults_on_and_keeps_an_off`; it travels in a settings bundle). The
  requirement was that off stops querying the system, not merely hides the two segments, so
  `app/main.rs`'s `start_resource_monitor(sample, show)` is no perpetual tick any more: an effect on
  `show` bumps a `generation` on every change, and only an on starts `resource_tick`, which waits
  1 s, returns unless the generation is still its own — `try_get_untracked`, so a disposed signal
  at shutdown stops it too — and only then does the one `refresh_processes_specifics` and
  reschedules itself: `activity_poll`'s shape. An off starts nothing and resets `sample` to
  `ResourceSample::default()`, so the next on shows nothing stale first; each on takes a fresh
  `sysinfo::System`, so its first CPU figure is not an average over the time it was off; and a
  write of the value the setting already holds (a settings import) is skipped by comparing with
  the effect's previous run, since Floem's `set` notifies regardless. With it off there is no
  timer and no `sysinfo` refresh at all, and `footer`'s two memos stop reading `resources`. `app_tests::hiding_the_resource_readout_stops_its_sampling`
  pins that order by its text — track the setting, bump, start only on an on; the generation asked
  before the refresh — and that `main.rs` holds exactly one `refresh_processes_specifics`, since a
  second sampler would not stop with the setting.
- `core/update.rs` — the auto-update state model: `resource.rs`'s neighbour in spirit, and the
  pure half of the Velopack plumbing in `schemaic-app`'s `update.rs`.
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
- `text.rs` — **`EditLog`**, the edits made to a text by revision, so that what keeps an
  answer about the text can take it forward without keeping a copy of the text to compare with.
  `record(pre, suf)` takes one edit as the bytes it left alone at the front and the back and
  bumps `rev`; `since(rev)` reads every edit after `rev` as one, `Since::Edit { pre, suf }` —
  the smallest front and the smallest back of them all, since bytes outside every edit's window
  are bytes no edit touched — or `Since::Unknown` for a revision older than the 512 edits it
  keeps (or one it has not reached). **A merged front and back can be smaller than what the two
  texts share**, so every `_edit` pass it feeds must be exact for any front and back the texts
  do share, not only the largest; each pass's equivalence test runs its corpus again at half
  of them for that reason. `the_edit_log_merges_edits_into_bytes_none_touched` holds the log to
  the texts: after a run of edits, each older revision and the text now agree on the merged
  front and back, which never overlap. The tab editor records its edits into one per pane
  (under `completion.rs`). **`common_affixes(a, b)`** (the bytes two texts share at the front,
  then at the back: where one edit turned one into the other, compared a 4 KiB block at a time —
  a `memcmp` — and byte by byte only in the block that differs) is the same question asked of a
  kept copy instead — what the old-text forms `sql_highlight::carried_lines_after`,
  `pairs::line_depths_after` and `sql::StatementIndex::after` find the edit with before calling
  their `_edit` forms. Only `carried_lines_after` is still reached outside the tests, by
  `FieldSql`: `SqlStyling` falls back to `line_depths_after` only without an edit log, and its
  one construction has one. **`count_newlines`** (eight bytes at a time, exact: a byte is zero
  after the XOR with `\n` exactly when it was one, and `((t & 0x7f…) + 0x7f…) | t` sets a
  byte's high bit exactly when the byte is not zero) is how every one of those passes finds the edit's line
  and how many lines it moved, per keystroke; a byte-at-a-time compare and count were most of
  those passes' cost (`common_affixes_find_the_edit` holds both to the naive form).
  `plural(n, one, many)`,
  returning only the noun form so a humanized count
  (`"1.2k"`) can be displayed while the singular/plural decision still follows the true `n` —
  a contract that reads as "returns the phrase" if you skim it, and the export note shipped as
  `Exported rows to employees.csv` on exactly that misreading, so a call site here owes the
  count as well as the noun. `human_count` (`1250` → `1.25k`), the **row-count** printer, shared
  by the grid's stats line and the properties surface so `200k` means one thing — and bound by a
  round-trip property: every string it emits must parse back through `model::goto_row_index`.
  **Both sides read their suffixes from one table, `COUNT_UNITS`** (`k`, `m`, `b` as powers of
  ten), where each used to spell them itself — and they are **symbols, never translated**: the
  stats line prints a count through one and the go-to box parses what the user retypes from it,
  so translating the printing side alone would print counts the box refuses. The decimal point
  between them is the same question for locale formatting, and it moved both sides at once:
  the stats line shows `human_count` through `i18n::count`, in the interface's marks, and
  `goto_row_index` takes the same `NumberMarks` — German's box reads `1,25k` as 1250 and
  `1.250` as 1250 grouped (`goto_row_reads_the_interface_language_s_marks`,
  `every_remarked_count_reads_back_in_german`). The group mark counts only where grouping can
  be, after a digit of the integer part with exactly three digits following. It was dropped
  anywhere once, so German read the English-written `1.25k` as `125k` and jumped a hundred times
  too far; now that is a miss (`a_group_mark_where_no_grouping_can_be_is_a_miss`). `NumberMarks`, `group_digits`, `group_number`
  and `remark` live here too (*Figures in the interface's marks*, under *UI conventions*).
  The property was stated in `human_count`'s doc and tested by nothing until
  `model::tests::every_count_human_count_prints_reads_back`, which reads back every printed
  count — the promotion edges `999_995` and `999_995_000` and a count past a billion among them —
  to within the print's rounding, and asserts every unit in the table parses.
  Not the namesake in `transcript.rs`, which buckets token counts differently on purpose.
  `completion_tail(complete, typed)` is the **inline ghost's** rule — the tail of `complete`
  past a case-insensitive prefix `typed` — and it walks `complete`'s own char boundaries rather
  than slicing at the lowercased length, because lowercasing is not length-preserving: the Find
  palette's first ghost sliced that way, and a table named `İzmir` plus a typed `i` landed inside
  a character and crashed the app. An empty `typed` answers `None` on purpose — a whole value
  painted into an empty box reads as one the box already holds — so the palette's
  `ghost_suffix`, which does show the selected row's name in an empty box, keeps that arm on its
  own side and delegates the rest. `field_suggestion(typed, options)` is the form field's half
  (`FieldCfg::suggest`, under `lib.rs`): the first option, in the caller's order, with a tail to
  paint, and `None` once `typed` already *is* an option in any case — typing `int` with `int` and
  `integer` both listed is a whole answer, and a ghost offering `eger` there would turn the Tab
  that moves on into the Tab that changes the value.
  **`cell_line(Option<&str>)` is the one definition of "one line" for a fixed-row table** —
  Compare results' and the Live monitor's: `NULL` for a null, every mandatory break (UAX #14
  BK/CR/LF/NL) as a space with a CRLF as one, cut at `CELL_LINE_CHARS` (120) ending in `…`
  (`a_cell_line_is_one_short_line`). Both tables are `VirtualItemSize::Fixed` stacks, so a value
  that shaped to two lines would paint across the row below (the Floem gotcha); the cut bounds
  what a column measures, while the comparison and the export still read the whole value. It
  lived as `result_diff`'s private `table_value` and, before that, as the monitor view's own
  `cell`/`is_line_break` — two copies of one promise, now one.
- `stats.rs` — table statistics: `TableStats`/`IndexStats`/`SchemaStats`, `format_bytes`
  (1024-based, IEC units), `format_age` — `span(secs)`'s English, the `Span` (one whole unit)
  the Properties panel words through `i18n::staleness_age` — and the honesty types. Every figure is an `Option`
  because the engines publish different facts — SQLite's sizes are exact, read from `dbstat`,
  and it has no row figure at all (under `schemaic-db`'s entry). **There is no
  `supports_table_stats` any more**: it was `false` for SQLite while nothing read SQLite's sizes,
  and once `dbstat` answered it would have been `true` for every engine — a constant with a
  function's name on it, which is the failure the capability rule names — so it was deleted,
  with `PropertiesState::Unsupported` and the app's two early returns behind it, rather than
  kept. `RowCount::{Exact,Estimate}` stops an estimate
  from printing as a fact, `Freshness` says *why* a figure may be stale (MySQL caches
  `information_schema` stats for `information_schema_stats_expiry`, a day by default;
  PostgreSQL's `reltuples` is only as fresh as the last `ANALYZE`), and `IndexStats::is_unused`
  flags an index only when the server actually counted zero scans — `scans: None` is "nobody
  was counting", never "drop this". The counts come from MySQL's `performance_schema`,
  PostgreSQL's `pg_stat_all_indexes` and SQL Server's `sys.dm_db_index_usage_stats`; SQLite
  keeps none. `count_rows_sql` builds the exact-count statement here
  rather than three times in the db crate, through the one quoter (`export::ident_sql`).
  **`TableStats::partitions` is each partition's share where a partition is storage inside
  its table** — `PartitionStats { name, rows, data_bytes, index_bytes }`, its `total_bytes` by
  `TableStats::total_bytes`' rule, found by `TableStats::partition(name)` — in the partitions'
  order, and filled on MySQL and SQL Server (under `mysql.rs` and `mssql.rs`). It is empty where
  partitions are tables (`schema::partitions_are_tables`, i.e. PostgreSQL), each partition
  having a `TableStats` of its own, and on SQLite, which has no partitioning.
  Filled by `Db::fetch_table_stats`; rendered by `ui/properties.rs`, the schema tree's
  size column, the results toolbar, the Truncate confirmation and a table drop's preview.
  - **Where a figure may be printed, and in what words, is decided here too** — the surfaces
    that print one hold no rules of their own. `catalogue_key` says where a statement's
    `qualifier.table` sits in the catalogue, because the engines that publish statistics
    disagree about what a qualifier *is* (a database on MySQL, a namespace on PostgreSQL and
    SQL Server) and
    getting it backwards reports one table's figures for another with nothing to show that it
    happened. **SQLite's answer is `None`, and stays `None` now that it has sizes**: it keeps no
    row estimate for the capped notice to print, so keying it would send a capped result's ask
    for a total into a `dbstat` walk of the whole file that cannot produce one.
    `SchemaStats::find` then resolves the name a *statement* wrote — unlike `get`,
    whose `None` namespace means "this engine has none", `find`'s means "the statement didn't
    say", so it matches by name alone and **only when exactly one table carries it**: an
    unqualified PostgreSQL name resolves through `search_path`, which the client does not know.
  - `rows_read` is the toolbar's whole row segment — figure, noun, and the `(capped)`
    notice — and the reason it is one function rather than three fragments the view assembles
    is that only the composition can decide whether the notice is still needed. It answers
    with a `RowsRead` — `All { read, n }`, `Capped { read, n }` or `OfTotal { read, total, n }`,
    the figures as `human_count` and `RowCount::label` print them, `n` the number the noun
    follows — which the grid words (*The string catalogue*); its `Display` is the English, and
    `rows_read_clause` is that `Display` as a function, kept for core's own tests. `OfTotal`
    names a total only when it is bigger than what was already read (`1k of ~400` reads as a
    bug rather than as the stale estimate it is);
    **`names_a_bigger_total` is that test, and it asks two things where it used to ask one** —
    bigger, *and rendered differently*. `human_count` keeps two decimals of its unit, so its
    resolution is 10 rows at `k` and 10,000 at `m`: a 200,004-row table read at the 200,000 cap
    printed `200k of ~200k rows`, which reads as "every row is here" — and, because a
    comparison *had* been made, without the `(capped)` that is the only word on the line saying
    otherwise. Same at every band: 5,004 at a 5,000 cap, 1,002,000 at a million.
    **Whether that comparison got printed is then what silences the word.** A total is in hand
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
    `truncate_prompt` is the Truncate confirmation's wording (its figure `confirm_figure`'s,
    which the interface words around) and `drop_scale` a table drop's row figure, and both
    name a figure only above `CONFIRM_ROW_FLOOR` (1,000) when it is an
    *estimate*: the point of naming one is scale, and InnoDB's sampled `TABLE_ROWS` is at its
    least reliable exactly below that — it reports 0 for a table holding a handful of rows, so
    *"Delete all ~0 rows in orders?"* would answer a question the user didn't ask, wrongly. A
    figure the engine actually counted is named at any size above empty.
    **`drop_scale` is a sentence in the preview, not a question before it.** A Drop used to
    raise a confirm first, worded by `drop_prompt` and titled by `drop_title`, and then
    the DDL preview, whose Apply asked the same thing again under the same warning — every Drop
    asked twice, and the user asked for the first to go. The one thing that confirm said and
    the preview's *"Drops the table and every row in it."* did not was the figure, so that is
    all that survived: *"By the schema tree's statistics, that is ~4.2m rows."*, added under
    the preview's warning by **`drop_warning(risks, rows, shape)`**, which the tree's Drop
    hands to `ddl_preview::preview_change_warning`. **It takes a `TableShape` rather than an
    `is_view` bool**, and only a `Table` is ever given a figure: a **view** owns no rows (they
    belong to the tables under it) and a MariaDB **sequence** is a counter. The boolean has two
    answers and the object has three, so a sequence once took the *table* sentence and the user
    was asked to confirm dropping "all N rows in it" about a counter
    (`a_drop_names_its_scale_only_for_a_table_with_a_figure_worth_naming` pins all three).
    **`drop_warning` also words a sequence's drop as one**: it is `Change::DropTable`
    (`stats::drop_change`, which `overlays::object_drop_change` calls — MariaDB's own `DROP
    TABLE` dropping a sequence), whose risk is *"Drops the table and every row in it."*, and the
    deleted confirm had been the only place that drop was worded for a sequence — so a
    sequence's warning is replaced with its own sentence, the reached value lost
    (`a_drop_warning_speaks_of_the_shape_it_drops`). **So is its change line**: the preview
    listed `DropTable`'s summary, *"Drop the table"*, over *"Drops the sequence"* until
    `stats::drop_preview` reworded both halves together — `drop_summary` over the lines,
    `drop_warning` over the risks — handed to `preview_change_warning` as one closure. Both
    halves are values: the lines are `ddl::ChangeLine`s, worded by the preview after the
    closure has run, so a sequence's is `ChangeLine::DropObject { kind: Sequence }` rather
    than English text, and the risks are `ddl::Risk`s — a sequence's warning
    `Risk::DropSequenceTable`, the figure `Risk::DropScale { rows }` — which the tree's Drop
    words with `ddl_preview::risk_text` inside the closure (under `ddl_preview.rs`). The
    property is pinned rather than the strings: for every shape on every engine that takes its
    drop, the line's noun is the warning's (`a_drops_line_and_warning_name_the_same_object`,
    R2-L6-02). And **every drop a Drop entry routes carries a warning** — every table-shaped
    row, each object kind's row drop, a numbered and a plain routine, an event, a database,
    a schema and each account kind — since `Change::risk_lines` ends in a catch-all `Vec::new()`
    and the confirm with the non-blank fallback that once caught an emptied arm is gone
    (`every_drop_a_drop_entry_routes_carries_a_warning`, R2-L6-01). **A PostgreSQL foreign
    table is the fourth shape** (`TableShape::ForeignTable`, under `schema.rs`): `drop_change`
    gives it `Change::DropForeignTable`, since the server refuses `DROP TABLE` on one, and
    `drop_summary`/`drop_warning` keep that change's own line and risk — which already name a
    foreign table — with no row figure, the rows not being here to count. Both pins above walk
    it, and the first is why its risk opens *"Drops the foreign table:"* — worded *"Drops the
    foreign table's definition."*, its noun was not the line's. **Truncate
    keeps its confirm-then-preview**: it is not a drop, and `truncate_prompt` is its question.
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
    is some), `index_facts` + `unused_note` (an index's own line; `index_fact_list` is the
    same decision as `IndexFact` values for the interface to word), and `count_row_state` — whose
    `None` is the state that removes the **Count rows** row rather than leaving a blank band
    where the control was. `IndexStats::cardinality_label` is the one reader of
    `IndexStats::cardinality`: it is an InnoDB sample of ~20 index pages, and printed through the
    *exact* branch it read as a measurement — `3,996,120`, on the clipboard as well as in the
    panel, four lines under a row count carefully marked `~4.21m (estimated)`.
  - **`TableStats::has_any` counts figures, and a creation timestamp is not one.** Measured on
    MySQL 8.4.11 against MariaDB 10.11: `information_schema.TABLES` returns every figure NULL
    for a view on both, *except* `CREATE_TIME`, which MySQL fills in. Counting `created` made
    `has_any` true there, so on MySQL the properties panel took its full `Loaded` arm instead of
    the arm written for this exact case — "A view has no storage of its own, so the server
    reports no statistics for it" — and printed two em-dashes under *these figures may be up to
    24h old*: a staleness claim about numbers the server never reports and never will, on one
    code path where two engines gave two different answers with nothing on screen to say which
    had happened. `to_markdown` diverged identically, so **Copy** put it in a ticket. The fix is
    in the predicate rather than in either surface, which is what keeps the two moving together;
    the timestamps are still shown by `options_section`, which has its own emptiness guard.
    **That last sentence was false of exactly the object the predicate is about, and both
    renderers had to be repaired for it to be true.** The panel's `!has_any()` arm did not call
    `options_section` at all and `to_markdown` early-returned after "The server reported no
    statistics for this table", so the MySQL view whose `CREATE_TIME` motivated the change lost
    the one fact the server *had* published about it — from the panel and from **Copy**. Both
    arms render `Created`/`Updated` now, `options_section`'s own emptiness guard meaning an engine
    that publishes nothing still adds nothing.
    `any_single_figure_counts_as_something` — the test that should have caught it — walks
    `rows`/`data_bytes`/`engine`/`indexes` and never `created`, which is why it shipped;
    `a_view_whose_only_figure_is_a_creation_timestamp_reports_nothing` is the one that fails
    against the old predicate, with its Markdown half beside it.
  - **Three `pub` functions were deleted here rather than left as a map of wiring that does not
    exist**: `SchemaStats::total_bytes`, `SchemaStats::tables` and `TableStats::unused_indexes`.
    Each was documented and tested and had no caller outside this module, and two of them read
    as live wiring — `total_bytes`' doc named "the 'this database is N on disk' figure", which
    the app does not show anywhere, so a reader of the module or of this document had reason to
    believe the schema tree carried one; and `unused_indexes` read as the panel's source for the
    unused flag while `properties.rs` and `to_markdown` each call `IndexStats::is_unused` per
    row, so a change to the rule there would have landed nowhere. That is the drift
    `index_facts`, `unused_note` and `shows_free` all exist to prevent. `TableStats::total_bytes`
    — the *table's* data plus indexes, which `storage_section` really does print — is a
    different function and is still here.
