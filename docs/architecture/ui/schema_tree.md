# `ui/schema_tree.rs`

`schema_tree.rs` — SCHEMA sidebar (`schema_panel` + db/table/column/key row builders + keyboard
nav).
**`SchemaTreeCtx` names what the rows reach instead of carrying a `Ui`** — `conn`, `schema`,
`ddl`, `overlay`, `schema_actions` and `table_colors`, beside the row state it already held.
It was the last context struct in the crate holding the root bundle, and `whole_ui_gate`'s
rule for those is under `lib.rs`: a ctx carrying `Ui` relocates it rather than narrowing it.
The cost is not only bookkeeping — this struct is cloned once per row, and cloning `Ui` bumps
about seven `Rc`s where five `Copy` bundles and one `Rc` bump one. **Two comments were wrong
about that field before it came off**, in opposite directions: the struct's own said it was
"for the row actions that open a modal" (true of `object_row`, silent on the rest) and
`BUDGET`'s said it was carried "for `table_colors` alone" (true of half of `table_node`). It
was reached in three functions for six things, one of them an `active_conn` the struct already
had a field for. Nothing checks a comment; the field list is checked by the compiler.
**Every word the panel writes itself comes from the catalogue** (`tree-*`):
the `SCHEMA` title, in the loading dots too; the two header tooltips; the search placeholder,
through `FieldCfg::placeholder_of`; *Loading*, *No tables*, `failed_row`'s *Failed to connect*
and *View*; `unread_row`'s text and *Read* link; and the count capsules. Those were a number
spliced before `text::plural(n, "table", "tables")` and are now one Fluent plural each
(`tree-count-tables`/`-cols`/`-keys`/`-partitions`); `the_count_capsules_read_as_they_did`
pins each against
`plural`'s output at 0, 1, 2 and 12. For the title, `widgets::section_title` and
`widgets::loading_dots` take `impl Into<String>` (the dots hold the prefix as an `Rc<str>`),
so every literal caller elsewhere still compiles. **Two things stay English on purpose**: the
key row's `UNIQUE`/`FOREIGN`/`INDEX` tags, which are SQL keywords, and the AI-explain prompts,
AI text being English throughout. That is why `failed_row`'s *View* opens the
`SchemaState::Failed`'s own `ModalError` as it came rather than building one: it used to make
`ModalError::server(msg)` from a bare string, which could carry the English or the wording but
not both, so the modal draws the wording (`shown`) and *Explain* sends the English (`text`).
**The filter box's text is the app's signal, `SchemaUi::schema_filter`, not the panel's**,
because it outlives the session (`UiState::schema_filter`); it was a panel-local
`RwSignal::new(String::new())` until the filter was persisted. The debounced `filter` mirror the
tree filters, highlights and re-expands off is unchanged, and a restored filter applies at once
because `widgets::debounced` seeds its output from the source's current value — seeded empty,
the tree would draw unfiltered for one debounce interval under a box that already said otherwise.
**An expanded `SchemaState::Unread` database renders `unread_row`** — "Single-user — not read"
and a *Read* link that calls `schema_actions.refresh_db`, which `begin_refresh` turns into
`Loading` and a fetch (why the load leaves such a database unread is under `schema.rs`'s
`unasked_reads`). **A press, not the expansion, and that is load-bearing**: the read is a
connection into a database that admits one, and the tree reopens with each node expanded as it
was, so reading on expansion would have the reload after `SET SINGLE_USER` take the slot on
nobody's say-so — the one thing the state exists to prevent. Like `failed_row` and `info_row` it
swallows a secondary click, a status row being a row.
**The panel's title is the only place a long catalogue read can be reported.**
`SchemaState::begin_refresh` deliberately leaves an already-loaded database's rows on screen
through a refresh, and the per-database `Loading` row only ever appears for a *first* load — the
case that was visible already — so on a database big enough for the read to take seconds
nothing anywhere said a re-introspection was in flight and pressing Refresh read as the app
having ignored the click. The title animates in place instead, `SCHEMA` → `SCHEMA...` through
`widgets::loading_dots`, which already takes the prefix; a label beside it would be a second
thing to lay out in a panel whose width the user sets. **It is delayed, not immediate** — the
effect arms `exec_after(schema::READ_NOTICE_DELAY, …)`, and the 400 ms is the same reasoning
`begin_refresh` gives for the rows showing nothing, applied to the header (*core::schema*, where
the constant and `report_read`/`report_read_since` live and are tested). The effect watches
`db_nodes`' `refreshing` signals and arms on the **rising edge only**, off its own previous
value: a second database starting while the first is still out must not queue a second timer for
the same notice. Any fall clears the notice immediately, without waiting for a timer.
**Nothing cancels a timer whose read has landed, so the notice is dated from the period on
screen rather than from how long the timer slept.** That same rising edge stamps an `Instant`
into a `began` signal, and the timer asks `schema::report_read_since(still, began, now)` — one
database landing at 150 ms and a second starting at 300 ms otherwise has the first timer fire at
400 ms over a read 100 ms old and announce it. A fall clears `began` beside `reading`, so a timer
that outlives its period has nothing to date from and says nothing. **And the timer reads before
it writes**: a floem signal never dedups, so a second one landing on the same `true` tore the
title down and restarted `loading_dots` from a plain `SCHEMA` mid-animation. The
timer's write is `try_update` because switching connections disposes the scope it was armed in,
and the read it was going to report with it. The busy branch restates `section_title`'s bold
face and padding on its own container rather than sharing a helper — `section_title` styles the
text and this styles the box the dots live in, so a helper would have to return a view either
way.
**A right-click on the tree's empty space raises its own menu** — `Refresh`,
`Users and privileges`, `Server variables` and `Create database`, all four about the *panel*
(or the server behind it) rather than about anything in it,
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
a live row that does nothing. A `BlankEntry` is `{ kind, disabled }` and holds no text:
`BlankKind::label()` draws it from the kind, out of the SCHEMA gear's own messages
(`tree-refresh`, `tree-users`, `tree-server-variables`, `tree-new-database`), since the two
menus are two homes for the same actions. `tree-new-database` was `tree-gear-create-database`
until both menus used it; under Create ▸ the entry is the noun alone, `tree-create-database`.
**The early return now asks whether the engine has anything here beyond `Refresh`** —
`databases || users || settings`, where it first asked `supports_database_editing` alone. **That
is the rule SQLite's menu is held to, and it now passes it**: having neither a database to
create nor an account to browse, SQLite raised **nothing at all** until
`server_vars::supports_server_vars` gave it a real second entry — its PRAGMAs — and it now
raises `Refresh` and `Server variables` (`blank_space_on_sqlite_offers_only_what_sqlite_has`).
The original point stands: a one-row menu on blank space reads as a misfire, and the gear still
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
where `Create database` is dimmed on either. `Server variables` follows it inside group 2 and is
gated the same way, being a read of the server too.
`blank_space_is_a_subsequence_of_the_skeleton` is
this menu's own half of `overlays::menu_order_gate`, which cannot see a menu built in this file,
and walks every dialect in `SqlDialect::ALL`, SQLite's menu included, placing each entry by
that gate's own `group` table rather than a copy of it;
see that gate for what it does and does not reach. The tests look entries up **by label**
(`labels`/`disabled`, the second panicking when the row is absent) rather than by index, so a
new entry cannot silently shift what an older assertion checks, and an assertion about a row
that is not there fails instead of passing by finding nothing.
The standalone objects hang off the same levels the tables do, in
`Types`/`Domains`/`Alias types`/`XML schema collections`/`Sequences`/`Synonyms`/`Functions`/
`Procedures`/`Events` folders after them
(`object_groups`/`object_group_node`/
`object_row`, over `schema::ObjectItem`, keyed by `ddl::ObjectKind::ALL` — one list, because
there were four copies of the kind array across the folder builder, its two filter predicates
and Find-Anywhere, and a kind added to three of them is a kind the palette silently cannot
find). An empty folder isn't rendered. Types and domains exist on PostgreSQL only; alias types,
XML schema collections and synonyms on SQL Server only (`DbSchema::tsql_objects`, under
`schema.rs`, drawn with `COPY` — not `BOOKMARK`, the palette's snippet glyph — `TYPE` and `BRACES`); sequences on both, as the one
`ObjectKind::Sequence`; functions and procedures on every engine with stored
routines; and events on MySQL — so a SQLite tree grows none of them. They are
scoped by `TableScope` for the reason
it exists — *flat* means the database has no schema level, not that its objects have no
namespace. **PostgreSQL's `Extensions` folder (`icons::PUZZLE`) is the database's, and so is
`TableScope::Database`, the third scope**: in a tree with a schema level, `object_groups`
under it lists only the database-wide kinds (`ddl::ObjectKind::is_database_wide`), and
`db_node` renders those folders **after the namespace rows**, at the database's child level —
also when no namespace survives the filter, since an extension's name can be the only match —
while `nav_rows` pushes them in the same place, after its namespace loop. In a flat tree the
folder is simply one of `Flat`'s. Never under the namespace it installed into: that is where
its objects went, not its address (under `schema.rs`). `object_group_nodes` takes a `database_level` flag to build that
level's folders (`the_extensions_folder_hangs_off_the_database`, which also searches for one).
**Whether there is a level is `schema_groups(schema, dialect)`**: none when the only
namespace `schemas()` returns is the engine's default (`schema::default_namespace` — PostgreSQL's
`public`, SQL Server's `dbo`; MySQL has none at all), one row per namespace otherwise —
**including a lone namespace that is not the default**. That case was flattened like `public`,
as "no choice to present", which hid the namespace's name and left its Drop no row to hang
from: a SQL Server database whose only objects live in `sales` listed them straight under the
database (`a_single_non_default_namespace_gets_its_level`,
`the_nav_walk_shows_a_single_non_default_namespace`). **An empty namespace counts too**, since
`schemas()` takes the reader's list as well as the objects' (`DbSchema::namespaces`, under
`schema.rs`): a schema just made with Create ▸ Schema, or emptied, is a node reading "0 tables"
with a "No tables" row and its context menu — Drop included, at the one moment T-SQL takes it
(Msg 3729 while anything is left inside). So `public` beside an empty `sales` now groups, while
a lone default still flattens. Nothing in `schema_groups` changed for it and no tree test pins
the empty case; `schemas_lists_a_listed_namespace_with_nothing_in_it` does, one level down, and
live `an_empty_schema_is_listed_compared_into_and_dropped` drops one. **The "No tables" hint
asks `DbSchema::has_objects_in`, which does not count a database-wide folder** — every
PostgreSQL database has `plpgsql`, so counting it removed the hint from every empty one — and a
flat empty database renders the hint with its Extensions folder beneath it. The dialect is the active connection's
(`active_dialect`), the render's and the nav walk's alike. Two filter rules follow from the level above being evaluated first: a database
and a namespace both survive a search that only one of their **objects** matches, or the
match would be hidden by the row that contains it. `nav_rows` carries the folders and their
leaves like everything else — it is the function that must stay bug-for-bug identical to
the render.
**`FolderItems` is why the header's count agrees with the rows under it**: a folder's surviving
items and how many of them there are travel as one value, because `items.len()` in the header
left a folder reading *"Sequences 200"* over the one row that matched. `object_group_nodes`
walks the items once to decide whether the folder renders at all, so the count is already in
hand and nothing walks them twice.
**`tables_shown` is the same repair one level up**, and `objects_shown`'s counterpart: the
namespace row's capsule read `schema.tables.iter().filter(by namespace).count()`, so filtering a
multi-schema PostgreSQL database for one table name left a namespace reading *"412 tables"* over
the two that matched. The children were right and the capsule above them was not, because the
two spelled the question separately. It derives `ns_hit` itself rather than taking it, since a
namespace whose *name* matches reveals all of its tables — the rule the children already applied,
and the half a caller cannot supply correctly by accident.
**A PostgreSQL partition hangs off its parent rather than sitting beside it**, and
`lists_here(schema, t, filtering)` is the one predicate both the render — `tables_shown` and the
flat database list — and `nav_rows` ask whether a table is listed at its level: not where
`DbSchema::lists_under_parent` says it belongs under its parent (under `schema.rs`). So the
namespace's capsule counts the tables listed there, its partitions not among them. The partition
comes after the parent's columns and keys, as a full `table_node` one indent level in, so it
keeps its own size badge, menu and columns; `table_node` takes the `Arc<DbSchema>` to find them
and returns `AnyView`, because it recurses — a sub-partitioned partition nests its own.
`push_table_rows` is the nav walk's same recursion in the same order, columns then partitions,
the key rows between them still not navigable (`nav_walk_hangs_a_partition_off_its_parent`).
**While a filter runs, every table is listed flat**, partitions included and none nested, so a
partition that matches is reached without its parent having to match. The counts row under a
table adds *N partitions* where it has any.
**Where a partition is storage rather than a table** (MySQL and SQL Server —
`partitions_are_tables` says no),
an open table lists each of its `TableInfo::partitioning`'s partitions after its keys as a
static leaf, `partition_row`: the Lucide `layers` glyph (`icons::LAYERS`), the name, the bound
muted, and its share of the size through `size_badge`'s `partition` parameter, which reads
`TableStats::partition(name)` rather than the table's figures (under `stats.rs`). It takes the
table's schema and hands it on, because SQL Server keys its statistics by schema: looked up
without one, no partition there ever showed a size. It opens
nothing, so like a key row it is not in the nav walk; the *N partitions* capsule counts these
rows too.
**`key_under(database, key)` owns which expansion-key families live under a database**, and it
is what "Collapse all" on a database row drops. That entry collapsed only the *tables*:
`collapse_db` retained `!k.starts_with("tbl:{db}:")`, while the same set also holds the `col:`,
`sch:` and `objgrp:` keys the very same `on_toggle` writes. So on MariaDB's `sakila`, whose six
routines give it a **Procedures** folder, the entry left that folder open with its rows on
screen; on a multi-schema PostgreSQL database, where the namespace level sits *above* the
tables, it left every namespace group open with its full table list rendered and read as doing
nothing at all — while the gear's identically labelled entry clears the whole set, so two entries
of the same name did different amounts of work. `db:` is absent from the list because the
database row itself stays open, `obj:` because an object leaf has no children to close, and
`keyrow:` because it never reaches the expansion set at all. Matching is on the
`{prefix}{database}:` boundary rather than on `{prefix}{database}` alone, so collapsing `shop`
leaves `shopify` alone (`collapsing_a_database_drops_every_key_under_it` asserts both halves).
It replaced `table_key_prefix`, which was re-exported from `lib.rs` and is gone.
**The set those keys live in is the *active connection's*, not the app's.** `expanded` is still
a `RwSignal<HashSet<String>>` this file reads with one-argument `contains` calls, but every key
in it is name-only, so a flat app-wide set opened `db:sys` on every MySQL-family connection at
once; `core::expanded` holds the `(conn_id, key)` rules behind it and the bug that made them
necessary, and the app refills the signal from `expanded::keys_for` on each switch.
**`chevron` takes the open state rather than deriving it**, and that is the same disagreement in
the disclosure triangle. Its key was `expanded.contains(key)` and nothing else, while every
consumer of that key ORs in a second term — `|| filtering` for a database or a namespace,
`|| force_cols` for a table, `|| (filtering && !parent_hit && !ns_hit)` for an object folder. So
for the whole duration of a filter every force-expanded node showed a **right**-pointing chevron
over children that were on screen, and clicking it flipped the glyph and changed nothing, since
with a filter running neither state of `expanded` alters what is rendered. `nav_rows` agreed
with the children, so the keyboard was right and only the glyph lied. Each caller now passes the
same expression its own children container uses, which is what makes one answer per node
possible at all.
**What the eye hides, it hides from every surface — and the rule is two predicates in
`core::schema`, not a filter each surface remembers.** `db_visible` answers "may a *list* show
this database": the tree's `dyn_stack`, `nav_rows`, the QUERY toolbar's selector menu and the
trigger that opens it, and **Find-Anywhere**, which had spelled it `!hidden.contains(…)` inline.
The palette has **two** branches and only the typed one was ever filtered: the empty-query
branch reads the search history, which is keyed by `conn_id` alone, so a hidden database's
remembered row sat in the list until a character was typed. That half is
`search_history::recent_visible`.
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
what this session can see), and so do `search_objects` without a `database`, which searches
that same overview, and `list_databases`, which is its names alone, while
`list_schema {"database": …}`, `search_objects {"database": …}`,
`describe_table`, `sample_rows`, `run_query` and `explain` answer in full — a named lookup is a fact, and half-filtering a fence `run_query`
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
row. **The Create entry asks `ddl::supports_object_creation`** and is absent where it answers
no: SQL Server's Sequences folder would once have opened PostgreSQL's sequence form, whose plan
`tsql_supports` refuses, and it now opens SQL Server's because the predicate computes from the
blank form the connection's dialect picks — the same predicate the database node's `Create ▸`
asks now (under `ddl.rs`). An object row's **Edit {kind}** was **absent, not dimmed**, for a
SQL Server object (`item.tsql()`) while there was no form behind it; it is offered there now,
dimmed as on any other object row on a read-only connection or where `is_editable_object`
answers no. Its **Drop** stays, gated as every row's is on
`object_drop_offered`.
**The Extensions folder is the exception to both.** Its Create is *Create extension*, which
opens `database_editor`'s picker rather than the object editor — there is no `ObjectDraft` for
an extension, so `supports_object_creation` answers no and the entry asks
`ddl::supports_extensions` instead; `menu_order_gate` lists its id,
`tree-folder-create-extension`, beside `Create {}`'s `tree-create-kind`.
An extension row has **no Edit entry at all** — absent, as SQL Server's objects' once was,
since no state of the connection gives it a form — and its Drop goes to the preview through
`ddl::drop_item` like any other row's.
**`table_glyph(shape)` is a table-shaped row's glyph and tint**, shared by the tree's row and
the Find-Anywhere palette's (`ResultIcon::Relation(TableShape)`, which replaced separate
`Table`/`View` icons), so the two cannot draw one object differently: a view wears
`TABLE_CELLS_MERGE` in the view tint, a PostgreSQL foreign table `EXTERNAL_LINK` in the table
tint, and a table or a MariaDB sequence `TABLE`. The tint is returned as a colour *function*,
for the theme-switch reason `table_node` gives.
- The **size column** (`size_badge`) puts each table's on-disk size at the right edge of the
  *panel*, from the same `core::stats` figures the properties modal shows. It answers the
  question that modal cannot — *which* of these is the big one — and is off by default behind
  SCHEMA gear → **Show table sizes** (`SchemaUi::table_sizes`, persisted as
  `UiState::show_table_sizes`). **That row is offered on every engine now.** It used to be
  absent on SQLite, behind `stats::supports_table_stats`, while nothing read SQLite's sizes and
  a row that visibly toggled while nothing changed was worse than none; once SQLite's sizes came
  from `dbstat` that predicate answered `true` for all four engines and was deleted (under
  `stats.rs`), and `gear_entries` pushes the row unconditionally, as it does the panel's other
  entries. A SQLite file over the `dbstat` cap leaves its slot at `Unavailable`, so that one
  database's column stays empty while the row stays offered. The panel and not the row, because those are not the same edge:
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
  on table rows only, so there is no column row beneath to collide with. MySQL's and SQL
  Server's partition rows (above) are the one exception, and **do not** take the same trade:
  a bound is the one thing on a tree row that is routinely long, and drawn under the size it
  was unreadable. So `partition_row` holds itself to `width(tree_row_min_w())` — the panel's
  width rather than the widest row's — and while sizes are on gives the bound `min_width(0)`,
  `text_ellipsis()` and a `theme::scaled(72.0)` right margin (the badge's 18px inset less the
  row's 8, plus room for a figure like *999.9 KiB*), so it clips short of the size. The name
  beside it does not shrink, so a partition name long enough still runs under. What runs
  underneath is also the chevron and the name, so `.pointer_events(|| false)` on the badge is load-bearing
  and not decoration: without it the badge, as the row's last child, won Floem's back-to-front
  pointer walk for the panel's full width and clicking a table stopped expanding it — with the
  column *off* too, because the empty state is still a full-width box. Keyboard nav kept
  working throughout, which is the tell that it was dispatch and not the toggle. The mechanism
  is under *Floem 0.2 gotchas*. `ConnNode::stats` holds one database's `SchemaStats` and is
  both the fetch's trigger and its guard: the app's `fetch_db_stats` fetches only nodes at
  `DbStatsState::Idle`, moving each to `Loading` before
  spawning, and settles a failure at `Unavailable` rather than retrying on every expand.
  **Two things ask it, and the slot is what keeps that from being two queries.** The size
  column's effect asks for the databases `schema_tree::wants_db_stats` answers for, with the
  column on — **expanded *and* visible, which is not the same as expanded**. It filtered on
  `expanded` alone while its own header said "only for the ones the user can actually see", and
  putting a database away with the SCHEMA eye prunes neither its `db:` key nor its node from
  `db_nodes` (the tree filters visibility at *render*). So an expanded-then-hidden database went
  on being asked for an `information_schema.tables` aggregate — the expensive one on a large
  MySQL instance — on every connect, every connection switch and every SCHEMA Refresh, for
  output rendered nowhere. The predicate lives beside the key builders rather than in
  `core::stats` because the expansion half *is* `db_key`'s, and asking that a second way is the
  drift those builders are gathered to prevent. A capped
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
uses — as does a builtin the engine calls bare (`CURRENT_TIMESTAMP`, under `rank.rs`), which
is why `accept_completion` appends `()` to a function row only when it carries no `insert` —
a routine inserted qualified or quoted carries its `()` in the `insert`, and
`rank::override_splice` places the caret between them. **A snippet row is the exception to
that caret placement**: `accept_completion` hands it to `begin_snippet(ed, comp, body, at,
dialect)`, which selects the body's first `:name` stop and sets `Completion::stops`
(`core::tab_stops`, under `tab_stops.rs`), or with no parameters clears them and puts the
caret after the body. The library's Insert ends in the same function (`editor_pane.rs`), so
the two snippet insert paths cannot place the caret differently; every other row kind still
places it by `override_splice`/`completion_insertion`.
**An accept replaces the whole word the caret is in**, from `word_start` to `word_end` — the
part after the caret too, and the `(` test reads past it — so accepting `customer_id` at
`cust|_id` no longer writes `customer_id_id`
(`an_accept_replaces_the_whole_word_the_caret_is_in`). Both edges run as far as the engine
continues a name (`sql::continues_name`), so `v$se` is one word and accepting `v$session` no
longer writes `v$v$session`; a byte that only continues a name never starts the word, so a
sigil that begins one — T-SQL's `@x`, `#t`, `@@x`, PostgreSQL's `$1` — stays before it, where
the sigil slots read it (`a_word_runs_as_far_as_the_engine_continues_a_name`).
**An accept is remembered per connection**: `accept_completion` records the taken row's text
against the tab's connection (`Completion::conn`, the editor's `active_conn`) in
`Completion::accepted`, and `recompute_completions` hands that connection's `bonuses` to
`RankInput::accepted` (under `accepted.rs`), borrowed from a thread-local `BonusCache`
(`ACCEPTED_BONUSES`, beside the catalog and index caches) rather than rebuilt per keystroke. Both accept routes — the key in `editor_pane.rs`
and a click on the popup — go through `accept_completion`, so neither can skip the record. The
cost is named: the record is a `Stored::update`, a `Replacing` save written synchronously on
the UI thread, once per accept and never per keystroke. `Ui::accepted` and
`QueryPaneParams::accepted` carry the store in from `app/ui_stores.rs`.
**A table row writes its alias after it** (`orders o`) where `intel::table_alias_at` finds the
slot for one (under `intel.rs`), behind Settings → Editor → *Table aliases* — on by default,
`UiState::completion_table_aliases` (`default_true`, pinned by
`table_aliases_default_on_and_keep_an_off`), `LayoutUi::table_aliases`. `accepted_table_alias`
gives one only to a plain table row: an FK join row carries its own inside its `insert`, a star
expansion replaces a range of its own, and inside a quoted name there is none
(`only_a_plain_table_row_is_given_an_alias_on_accept`). The gatherer clears each
`JoinTarget::aliased` while the setting is off, so `rank.rs` needs no flag of its own.
`theme::completion_table_aliases` is read untracked at the moment a list is gathered or a row
accepted; nothing is drawn from it, so its setter repaints nothing.
**No list opens inside a string literal or a comment, not even on Ctrl+Space**:
`recompute_completions` asks `intel::caret_in_literal_or_comment` of the caret's statement
before it reads the clause, ahead of the FK join row and the ranked list alike, and closes the
popup where it answers. `WHERE name = 'jo` offered columns, and Tab spliced one into the
literal. A quoted name is not a value, so `"Ord` still completes (why the gate is not
`caret_touches_noncode` is under `intel.rs`). It gates the list only: the ghost is published
above it, and `predict` and `recall` refuse such a caret on their own terms. **The one value
it lets through is an enum's**: inside the gate, `intel::enum_value_slot` is asked first, and
where it answers and `rank::enum_value_suggestions` — over the memoised `SchemaIndex` and the
statement's scope — is non-empty, the popup opens on those rows (anchored at the caret,
selection 0, `give_way(true)`) instead of closing. Any other literal or comment still closes
it. `SchemaIndex::build` fills `enum_types` from the in-scope databases' `DbSchema::enums`.
**Nor does one open on a number**: `numeric_literal_at` closes the popup where the word before
the caret still reads as a number (`reads_as_number`: digits, then an optional exponent, or
`0x`/`0b` and their digits — `1e` and `0x` included, being the way to one), straight after the
literal gate above, and on Ctrl+Space
too. **After that gate, not ahead of it**: inside a string a digit is no number, and the enum
members it lets through may begin with one — placed first, it closed the list on `size = '7`
against `ENUM('7x','10x')`
(`an_enum_member_that_begins_with_a_digit_is_still_offered`). `word_start` walks word
bytes, which include digits, so `WHERE id = 7` handed the fuzzy matcher the prefix `7`, a
subsequence of the builtin `UUID_V7`; the list opened, and Enter, meant to end the line,
replaced the number with `UUID_V7()` (`typing_a_number_offers_nothing`).
`sql::is_word_start` excludes digits for the same reason. The exception is load-bearing: a
word after `` ` ``, `"`, `[` or `.` is a name that happens to start with a digit — MySQL's
`` `7days` ``, `t.1col` — and still completes
(`a_quoted_or_qualified_name_may_begin_with_a_digit`). **So is a word no number has the tail
of**: MySQL takes an unquoted name that begins with a digit and is not all digits, and a gate
on the leading digit alone closed the list on `2f` over a table `2fa_codes`
(`a_name_that_begins_with_a_digit_is_still_offered`).
**`Completion::word_complete` carries `rank::Ranked::completes_a_word` to the key handler**
(under `editor_pane.rs`). `set_items` resets it to `false` — every path that fills the list goes
through there, so the FK join row or an emptied list never inherits the last ranked list's
answer — and the ranked path sets it straight after.
**`recompute_completions` fills `RankInput::flavour`, and it needed no new signal to do it**:
the flavour is stamped on the loaded `DbSchema`, and this function already destructures
`active_db` and `db_nodes`. It asks through `table_designer::db_flavour`, which was narrowed to
take the node signal rather than a `SchemaUi` precisely so autocomplete and the table designer
share **one** lookup instead of two spellings of the same `db_nodes` walk. No active database
means no schema to read it off, and `ServerFlavour::Unknown` offers everything — the reasoning
is under `core/intel.rs`'s `is_offered_builtin`.
**The `SchemaIndex` is memoised per UI thread beside the catalogue, and it was the half that
never got it.** `index::build` walks every loaded database, every table and every column,
allocating a fresh `ColMeta` per column plus three `HashMap`s, and `recompute_completions`
called it directly — a function that is deliberately undebounced (`editor_pane` schedules it at
`Duration::ZERO`, one tick, only so the caret has settled), so it ran on effectively every
keystroke of the ordinary case, typing an identifier leaving a non-empty prefix. That exact
cost had been found and fixed once, for `Catalog`, with a cache keyed on `Arc` identity; the
index kept paying it. `schema_index` is the lookup and `CachedIndex` the entry — the nodes, the
hidden set, the active database and the built index — rebuilding only when one of those has
moved, and comparing schemas by `Arc::ptr_eq` rather than by value, since identity is what a
re-introspection changes and so nothing has to be invalidated by hand. `LoadedNode` exists so
the snapshot is taken out of the signals **once**: reading `node.schema` twice could key an
index on a schema it was not built from.
**`Completion::stmts` holds the pane's `sql::StatementIndex`** (`StmtIndex`, the text revision
and dialect it was built for beside it), and `stmt_index(comp, sql, dialect)` is the one way
in: the held index when the revision, the dialect and its `text_len` all match, the held one
taken forward (`after_edit`, with `edits.since(rev)`) when it is not, a new one otherwise — a
new dialect, a revision the log no longer keeps, or a merged front and back longer than either
text. The revision is `Completion::edits`, the pane's `text::EditLog`, which the editor's
`.update` fills from every delta (`RopeDelta::summary()`: the interval's start is the front,
`base_len` less its end the back) **before** it sets `query`, so anything `query` sets off
already sees the new revision. It used to keep a copy of the text beside the index and compare
it with `==` on every call, which was a document's copy per keystroke and a document's compare
per caret move. Four readers ask it — `update_signature_help` per caret move (it was
`sql::statement_range`, a walk to the caret), `recompute_completions` per keystroke
(`writing_range`), `prediction_at` per keystroke at a lone caret (the gate, `predict` and
`Recall` through their `_indexed` forms, and the own tab's ranges for `Recall::from_ranges`),
and the run outline's effect in `editor_pane.rs` — so the walk each made becomes a search, and
a keystroke rescans about its own statement (the costs under `sql.rs`).
**`Completion::temps` sits beside it**, an `intel::TempTableCache` holding the document's
temporary-table definitions under the same revision, dialect and length. On a miss the
statements come from `stmt_index(...).statement_ranges`, which the caller has just taken
forward to this revision for its writing range, and the cache is updated with `try_update`
with the index already in hand, so no signal is written inside another's update. A caret move
or a re-rank in a `#` table's column slot is the fold alone (the split is under `rank.rs`).
**`index::build` had no test of its own**, which is the other thing to know about it: it is a
pure function over a slice — no signal, no `Editor`, nothing that needs a mounted view — while
the hidden-database rule it enforces was pinned only for the *catalogue* half (`catalog_tests`),
a different function over a different structure. `index_tests` covers what it decides: a hidden
database contributes nothing at all, not its name, not its tables, not its columns; the **active**
database contributes even when it is hidden, which is what keeps a tab usable after its own
database is hidden; only the active database reaches the unqualified pool while the qualified
maps hold every database, which is what an explicit `otherdb.table` completes from; a foreign-key
column is marked and its siblings are not, that flag being the FK tint's source; two databases
sharing a table name merge their columns once each, exercised on both halves because the
`entry.is_empty()` fast path exists to keep the ordinary case out of the quadratic merge and only
the *second* table takes it; and the database list dedupes case-insensitively, so two connections
onto one server spelling a database differently are one row in the popup.
**A SQL Server synonym is offered as a table** — it was not offered at all, being held in
`tsql_objects` apart from `tables`. `index::build` reads `DbSchema::synonyms` (under
`schema.rs`) and puts each into `tables_by_db`, and into `tables` where its database is in
scope; one resolved to this database's own table or view gets that table's `ColMeta`s in
`columns_by_db` and `columns`, and any other gets none rather than a guessed table's
(`a_synonym_is_offered_with_its_targets_columns`, red before). **Built from the resolved table
itself** (`col_metas`, the tables loop's own rule), not looked up under the target's
`(database, name)`, which holds whichever same-named table was indexed last: a synonym over
`dbo.docs` beside a later `sales.docs` was offered `sales.docs`' columns, which diagnostics then
squiggled — and **merged** into a same-named table's entries rather than written over them,
as the diagnostics catalogue does (`a_synonym_is_offered_its_resolved_tables_columns`,
S4.2-L1-05, red before).
**The in-scope database's tables are indexed by schema too** — `columns_by_schema` and
`tables_by_schema`, from `TableInfo::schema`, which PostgreSQL and SQL Server fill. On those
engines `sales.orders` reads as `TableRef.db = sales`, a namespace in the tab's database, so the
`columns_by_db` lookup `rank`'s `cols_of` makes found nothing and `s.` offered no columns; it
now falls back to the schema's map, and a `sales.` qualifier offers the schema's tables after a
database of that name has had its turn (`a_schema_qualified_table_offers_its_columns`,
`the_tabs_database_is_indexed_by_schema_too`, both red before). Limited to the in-scope database
as `intel::Catalog`'s `schema_qualified` is, since a `schema.table` never crosses a database.
Beside each `tables` push, `index::build` records the table's namespace under its lower-cased
name in `SchemaIndex::namespaces`, which is how `rank` decides that a table no bare name reaches
inserts qualified (under `rank.rs`). A synonym pushed into `tables` gets no entry and so always
goes in bare. No `index_tests` case covers that line.
**The inline prediction is asked before the popup, and a reasoned one outranks it on an empty
word.**
`recompute_completions` asks `prediction_at` — `intel::predict` over the *offer* catalogue
(`build_offer_catalog`), the one the FK join row reads, so a hidden database is no more predicted
from than offered — and publishes the answer through `inline_diff::set_ghost` **before either
one-shot can return**. `suppress` is set by an accept, and the next prediction is wanted
straight after one: Tab on a `BY ` ghost leaves `GROUP BY `, whose grouping columns are the next
thing to offer, so a ghost asked after the one-shot would break that chain at its second link.
**The offer catalogue is built only where the gate passes** (`intel::may_predict_indexed`, the
caret's statement read off the pane's index — under `intel.rs`):
building it is the costly half of a keystroke's prediction, and most carets — mid-word, in a
string — are ones `predict` would refuse before reading it. The gate decides the reasoned ghost
alone; `recall` is asked whenever that is `None`, the gate's refusal included, and runs its own.
**Where `predict` has nothing, `prediction_at` asks `recall`**, which runs
`intel::Recall::at_indexed` once and asks `Recall::from` of three sources in the order they are trusted, the first to answer
winning — an order `runs_come_before_snippets_and_snippets_before_tabs` pins through the real
`recall` (red with runs and snippets swapped). **First the runs `recallable` keeps, newest first**: this connection's
(`HistoryEntry::conn_id == ctx.conn_id`), leaving out `Outcome::Failed` ones — a statement the
server refused is not one to offer again; an `Unknown` run, cancelled or still going, is
offered — and, where both the run's `HistoryEntry::database` and the tab's active database are
known, only a run in that database, compared by the engine's own case rule
(`history::same_database`, under `history.rs`). The connection alone let a statement run in
one database be offered in another; where either side is unknown the run is kept. That rule is
`same_place`, which the open tabs below go through too
(`a_run_is_remembered_only_where_it_ran`, red against a stub of the old filter, and asking it
through `same_place` since). Both take the dialect for it: they used to compare with
`eq_ignore_ascii_case` on every engine, which on PostgreSQL — where `Sales` and `sales` are two
databases — offered one's statements in the other
(`an_open_tab_is_remembered_from_only_in_the_same_place`). The log arrives as
`CompletionCtx::history` (`ui.history.entries`, every connection's, through
`QueryPaneParams::history`). **A statement carrying a credential is offered from none of the
three sources**, and not because `history::push` declined to record it: this passage once said
so, while the snippets and tabs below — which `push` never sees — were read as typed and
ghosted a password. The gate is `Recall::from`'s own, asked of any text (under `intel.rs`).
**Then the user's own snippets that apply here** (`snippet_recallable`, over
`CompletionCtx::snippets`): `Source::User` alone — a built-in is a template nobody wrote, not a
statement anyone ran — and only where `snippet::applies`, so one scoped to another connection
or engine is not offered
(`a_snippet_is_remembered_from_only_if_it_is_the_users_and_applies_here`). These are the saved
queries; the source was first asked for as "saved favourites", but `favorites.json` holds
favourite *databases* (`favorite::FavoriteRule`), not queries. **Last the open tabs in the same
place** (`CompletionCtx::tabs`, the tab's `conn_id` and `database` through `same_place`;
`an_open_tab_is_remembered_from_only_in_the_same_place`) — statements written but not run, the
tab being typed in among them: its current statement offers nothing, being what was typed, and
each of its others may. **Each tab's text is read inside its own signal**
(`try_with_untracked`, its `conn_id` and `database` through `try_get_untracked`), not cloned
out, since this runs on a keystroke over every open tab; and the `try_` is what lets a tab
disposed under it answer nothing rather than read a dead signal. The three predicates are under
test; where `recall` is called and its reading of the tab signals live in the view and are not.
**Remembering has its own switch**, Settings → Editor → *Predictions from past statements*
(`UiState::editor_remembered_predictions`, `default_true`, so on for a new install and a file
from before it alike, and independent of *Inline predictions* —
`remembered_predictions_default_on_and_keep_an_off`), carried as
`LayoutUi::editor_remembered_predictions`, `QueryPaneParams::remembered_predictions` and
`CompletionCtx::remembered`; `recall` answers nothing when it is off. Off keeps past statements
off the screen — a screen share's among them — while the reasoned ghost stays. Its *value* is
independent; its *switch* is not: with *Inline predictions* off, `prediction_at` returns before
`recall` is asked, so the row is greyed out and leaves the Tab ring until predictions are back
on, keeping whatever it held (`settings::focusable_toggle_row_when`, under `settings.rs`; a view
decision, untested).
**Which of the two answers, and whether it is shown at all, is `pick_prediction`'s** — the
reasoned one, else the remembered one, flagged as such — and **neither when it holds a line
break**. `InlineDiffDoc::ghost_on` never drew one, but it was published all the same, so Tab
still inserted it, Escape was spent on it and the popup was held shut for it: an offer nobody
could see, which a `GROUP BY` list whose expression holds a string literal with a real newline
produced (`a_prediction_is_shown_only_if_it_can_be_drawn`, red before). A ghost published
before a Ctrl+K preview opened is the other one nobody can see, and that is the keys' to
refuse: they ask `inline_diff::shown_ghost` rather than the slot (under `editor_pane.rs`).
A forced recompute (Ctrl+Space) asks for the list, so it shows no ghost beside it.
**Nor is a ghost asked anywhere but a lone caret** — `lone_caret`, the cursor's offset only
when it is one caret with nothing selected: Normal mode's offset, Insert mode's one caret
region, never Visual. A ghost used to be asked at `cursor.offset()`, which with several carets
is the last-inserted region's, and taking it there collapsed the rest; and an edit that left a
selection — Tab indenting a block — got a ghost at the selection's end, which the next Tab,
meant to indent again, inserted, dropping the selection (`a_ghost_sits_only_at_a_lone_caret`,
red before). `lone_caret` is also what both keys and the clearing effect read (under
`editor_pane.rs`), so the three cannot disagree about which caret a ghost belongs to.
**The rest of a prediction taken a word at a time, or typed into, is kept, not asked for
again**: a ghost `keeps_ghost` accepts is published as it stands, ahead of `prediction_at`,
because asked at the new caret the words already taken often predict nothing — after
`JOIN customers ` there is no alias, so no `ON` ghost — and mid-word nothing is asked at all,
`intel::predict` and `predict_from_history` both refusing a caret without whitespace or `(`
before it. **It asks for the caret *and* the document's length** the rest was published at
(`kept && doc_len == len && offset == caret`): the offset alone let a stale rest survive an
edit that moves no caret — a forward Delete at the caret changes what follows it — where now
that edit asks afresh (`kept_ghost_tests`;
`a_kept_rest_is_dropped_once_the_text_or_the_caret_changes` was red against the offset-only
rule). A forced recompute still shows none, and any caret move still clears it — unless the
move typed the ghost's own next letters (under `editor_pane.rs`). `ghost_at` is the one place
an `EditorGhost` is built, with its line and column and the rope's length as `doc_len`, for
`prediction_at`, that key and the clearing effect alike; a kept rest carries on the
`remembered` flag of the ghost it was cut from.
**Typing into a ghost leaves the rest of it** (`typed_through`) — the fish shell's and the code
editors' rule. A ghost used to vanish at the first letter, the caret no longer being at its
offset and neither predictor answering mid-word, and came back only after the next space.
`typed_through(g, caret, rope)` answers the rest only when the edit was exactly the
ghost's next characters typed at it: the caret ahead of `g.offset` by k > 0 bytes, the document
grown by exactly k since the ghost was published, the text between equal to `g.text[..k]`, and
something left over. **The comparison is exact, case included** — Tab would write the ghost's
own text, so a `b` typed at a `BY ` ghost is not a step towards it — and a k that splits one of
the ghost's characters answers `None`, as does a caret that moved without the document growing
by as much (`typing_into_a_ghost_leaves_the_rest_of_it`, red against a stub returning `None`;
`anything_but_the_ghosts_own_letters_drops_it`). **It takes the rope, not the typed text, and
every check that can refuse — the length, and both `g.offset` and the caret on a codepoint
boundary — comes before it slices.** The cursor effect used to slice `g.offset..caret` itself
and hand over the result, and an edit *elsewhere* that grew the text by as many bytes as the
caret moved — Ctrl+/ putting `-- ` before the line, Format Code — left `g.offset` inside a
multi-byte character: xi-rope panics on such a cut, and this was the UI thread, a High release
blocker (`an_edit_elsewhere_that_splits_a_character_drops_the_ghost_without_slicing`, red
before with xi-rope's own panic). Hoisting
the slice back into the caller, or putting it ahead of the boundary checks, brings it back.
**Only a reasoned ghost takes the empty word's place** (`ghost_displaces_the_list`). With the
word at the caret empty and an `intel::predict` ghost showing, the popup does not open: that is
a slot the list opens on its own guesses — `auto_show`'s columns after
`WHERE`/`ON`/`BY`/`SET`/`HAVING`/`SELECT` and tables after `FROM`/`JOIN`/`INTO`, and the FK
join row — and two offers at one caret would leave Tab meaning whichever happened to be open.
So where the whole join is predicted, the table list after `JOIN ` waits for a typed letter.
**A remembered ghost does not displace the list**: it is one past statement, and letting it
take the slot made the table list after `FROM ` and the columns after `WHERE ` stop opening by
themselves once any `SELECT * FROM x …` had run. Where the list then opens — the FK join row or
the ranked list — the remembered ghost gives way instead (`give_way` clears it in the same
recompute), so the caret still carries one offer, not two
(`only_a_reasoned_ghost_displaces_the_list`, red against a stub of the old rule; where
`give_way` fires needs a live `Editor` and is not unit-tested).
**A kept ghost displaces the list at any word**, remembered or not: it is the offer being
followed, and typing into it is exactly what opens the list — `typed` is set and the word at
the caret is the letters just typed — so under the empty-word rule alone the list opened on
them and an open list owns Tab (`a_kept_ghost_displaces_the_list_at_any_word`, red against the
old rule). The displacement returns before either list is built, so a kept remembered ghost
never reaches `give_way` either. Ctrl+Space still brings the list, join row included. `CompletionCtx::predictions` carries the `editor_predictions` setting into
the *asking*, for both kinds of ghost, and `CompletionCtx::remembered` the
`editor_remembered_predictions` one, for the remembered kind; each is read only there — neither
is the switch on its own: a ghost already drawn when either setting goes off is cleared by an effect under
`editor_pane.rs`, where before it stayed and Tab still took it until the next edit.
**The selected function's summary is a footer under the list, not a column beside each row** —
a column would widen the popup to the longest description in it. `selected_summary` reads
`comp.sel` and `comp.items` tracked, so the footer follows the keyboard, and answers only for a
`SuggestKind::Function` row, through `intel::builtin_summary`: the catalogue signature help
reads, so the footer never describes a function the engine has not got. One line, ellipsized,
muted, at `completion_annot_size`, under a top border, and hidden when there is nothing to say.
`natural_width` does not count it — it ellipsizes to the width the rows chose. **It is one more
row to the placement, in both places that ask**: the scroll's own `popup_placement` call and
the outer positioning style's, with `completion_row_h()` taken off the scroll's `max_height`.
That is what keeps a popup flipped above the caret clear of the caret line — the flip is placed
by the predicted height (under *Floem 0.2 gotchas*), so a footer counted in one call and not
the other is a box taller than the space it was placed in. No test covers the footer; it is a
view.
**A row's own words are said in the interface's language** — the FK join row's *foreign key*
and the star expansion's *expand \** and its column count, which reached the popup in English
whatever the language. `shown_words` answers a row's text and detail: core's, or for a row
carrying `rank::RowWords`, the catalogue's (`completion-foreign-key`,
`completion-expand-star`, and `completion-columns`, a plural whose `$n` is the count through
`i18n::figure`). **The row view and `natural_width` both read it**, so a longer translation is
the width the box is given; the row's exhaustive `Suggestion` destructure discards `text`,
`detail` and `words`, so core's English is not in scope where the row is drawn. `english_says_every_row_word_as_core_does` holds it to
core's English — `join_condition_row`, the expand row at 1, 3 and 1,200 columns, and a plain
row — and was red with *foreign-key* hyphenated in `en-GB`.
