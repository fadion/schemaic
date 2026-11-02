# `ui/perf_view.rs`

`perf_view.rs` — the **Performance** view (`perf_overlay`), over `core::workload`: why a server
is slow, beside Server Activity's who-is-running — the statements it has spent most on, the
indexes worth a look, and the largest tables and indexes, for the connection and database it was
opened on. It is the app half of the roadmap's *Workload and performance insight*, the MCP
server's four performance tools being the other (`cli/mcp.md`), and **it ranks with
`core::workload`'s functions over the same `Db` reads those tools call**, so a person and an agent
looking at one server are told the same thing — the same ranking, not the same length: the app
asks for 50 statements, 50 tables and indexes and 50 missing indexes, where the tools default to
`PERF_ROWS` (20). **Nothing here writes**: *Drop* on an index finding and *Create index* on a
SQL Server suggestion each go through the table's draft, `ddl::diff` and the DDL preview, and the
statement that would turn a missing statistics source on is shown, never run. Modelled on the
Server variables viewer (`ui/server_vars_view.md`), and mounted in the workspace group directly
after it; on `modal_backdrop_gate::PAINTS_A_BACKDROP`.
**Three entry points, one door.** `GearKind::Performance` in the SCHEMA gear and
`BlankKind::Performance` in the tree's blank-space menu, each directly after `Server variables`
and before `Create database` (skeleton group 2, beside `tree-server-variables` in
`overlays::menu_order_gate::group`), and the palette's `performance` command
(*Performance*), an `instant` command. The gear row — and the blank-space row, wherever that
menu is raised at all — is **present whenever a connection exists and asks no capability** —
every engine has something to show, SQLite its index health and its
`dbstat` sizes with no statement history, which the view says — and dimmed on a **down**
connection but not on a read-only one: it is a read, and its *Drop* and *Create index* land in the
preview, which carries the read-only refusal. That is a departure from the row above it, which
asks `server_vars::supports_server_vars` although every engine answers `true`, so that a fifth
engine has to answer it rather than inherit a row; this one would be inherited. The palette
command is omitted with no saved connection, beside *Server Variables* and for its reason — a row
that closes the palette and does nothing reads as broken — and is not dimmed on a down
connection, leaving that to the fetch. All three call
`open_for_active(conn, overlay, tabs_ui.active_db)`, which resolves the active connection's
name and dialect (a no-op with none), resets the list to Statements, the order to Total time and
the state to `Loading` on the way in — `server_vars_view::open_for_server`'s rule, so a second
opening never flashes the last one's list — and sets `OverlayUi::perf` to a `PerfTarget {
conn_id, name, database, dialect }`, captured at open for `VarsTarget`'s reason. The database is
the **active tab's**, an empty one filtered to `None`, and it is in the title
(*Performance — <connection> · <database>*). `PerfCtx` holds `OverlayUi`, `ConnUi`, `SchemaUi`,
`DdlUi` and the one fetch, `SchemaActions::perf`, and is built at its one call site in
`modals.rs`.
**The fetch effect keys on `(target, view, order)` and lives above the `dyn_container`**, the
viewer's rule — an effect created inside the child runs again on every rebuild of it — comparing
against its previous value because `create_effect` has none of its own; the order counts only
while the list is Statements, the one list it ranks. **Indexes and Sizes are one database's, so
with none chosen the effect sets `PerfState::Failed(perf-no-database)` instead of asking**, and
Refresh asks the same question first, so neither path reaches the app with a request that has
nothing to read; the app's `(_, None)` arm, an `Err` with an empty message, is there only for a
request that arrives anyway. Statements take a `None` database — `fetch_top_statements` resolves
it to the connection's own (`db/overview.md`). The app's `perf_fetch` (beside `server_vars_fetch`
in `main.rs`) has that function's shape: a generation (`OverlayUi::perf_generation`) claimed
before the round trip and compared on landing alongside the target, the list and — on Statements
— the order, so Statements → Indexes → Statements on a slow server is three fetches and only the
last may write. Statements are `Db::fetch_top_statements(database, order,
DEFAULT_STATEMENT_LIMIT)`; Sizes are `fetch_table_stats` ranked by `workload::largest(&stats,
PERF_VIEW_ROWS)` (50, a `main.rs` constant); Indexes are `fetch_schema` and then
`fetch_table_stats`, swept by `workload::index_health` and `usage_counted`, and — where
`supports_missing_index_hints` — `fetch_missing_indexes(Some(db), PERF_VIEW_ROWS)`, **whose
failure costs only that section** (`PerfData::Indexes::missing_error`), as a refused
usage view costs only the usage figures. It adds no deadline of its own, and the `fetch_schema`
token is never fired, so closing the view abandons a read rather than cancelling it; the landing
guard is what keeps the late answer off the screen. `PerfState` is `Loading`, `Loaded(Rc<PerfData>)`
or `Failed(String)`, and `PerfData` is `Statements(StatementReport)`, `Indexes { findings,
counted, missing, missing_error }` or `Sizes { tables, indexes, per_table }` — all in `lib.rs` beside
`PerfTarget` and `PerfView`.
**An absent source is said, never drawn as an empty list** — `core::workload`'s rule, and the
reason `StatementReport::Unavailable` reaches the body whole. Its reason is the interface's words
(`perf-src-*`), and **each must read in English exactly as `Unavailable::reason()` does**, since
that is what the MCP tool tells an agent about the same server
(`english_says_every_missing_source_as_core_does`, `NoPermission`'s `$error` included); where
`enable_sql()` has a statement it follows under *To turn it on, run this in a query tab:*, in
mono, with a *Copy* beside it — copied to the clipboard, never run from here. A source that is on
and has recorded nothing says so. Each statement row is `history::preview`'s one flowing line,
held to two lines by a `max_height` and `clip()`, with its figures under it joined by middle
dots — total, mean, calls, rows where the source counts them, and the database where it records
one. A figure of a millisecond and up is `i18n::took` over `history::took` of the rounded value;
below one it is `perf-sub-ms` at two places, because rounded it would read `0 ms`, where a lookup
lives. Calls and rows are plurals on `i18n::count` (`the_counted_figures_read_in_both_numbers`).
A ready list ends *Counted since …* where the source dates its counters. **The statement's text
is copied by a right-click only** (*Copy statement*, on the generic `popup_menu` channel at the
pointer); the rows have no keyboard route to it.
**Index findings each carry a *Drop*, and SQL Server's suggestions a *Create index*, and both are
draft edits**: `table_designer::preview_draft_edit` with an edit that removes the index from the
draft by name, or adds the suggestion with `MissingIndex::add_to` — the same edit
`workload::suggestion_ddl` makes for the MCP tool, so the two front ends create one index — and
the statement the user reads is `ddl::diff`'s, through `preview_of`, so *generated DDL is never
emitted from a second differ* holds (`invariants.md`). **The buttons are offered only where the
edit would act (`editable`)**: the target
connection must be the active one, and `table_designer::loaded_table` must find the table in the
tree. Both halves are load-bearing. `preview_draft_edit` returns silently without a loaded table
— while the database is unexpanded, or mid-refresh — so a button there would be a press that does
nothing; and it reads the **active** connection's tree and takes the active connection's id and
dialect (`edit_ctx`), so after a switch a same-named table on another server would be the one
previewed — the case `copy_table_view` refuses with a sentence. The question is asked when the
list is built, not live. The draft is seeded from the tree's model, not from the `fetch_schema`
the findings came from. Findings are worded per `IndexProblem` with their size (`i18n::bytes`)
where reported; under them is the usage caution, or — where `usage_counted` is false — the line
saying no index can be called unused. The missing-index section is drawn only where
`supports_missing_index_hints(dialect)`: its read's error, *None recorded.*, or the suggestions
with their uses and estimated improvement and a closing line to check each against the indexes
above. Sizes lists the largest tables, with an estimated row count where there is one, then the
largest indexes; with that list empty it says **which empty** it is — the engine reports index
sizes per table, or the database has no indexes — from `PerfData::Sizes::per_table`, which the
app fills from `workload::index_sizes_per_table_only`, as the MCP `table_sizes` reply does.
**It stays open behind the preview it raised, and hides itself while that is up** —
`perf && preview.is_none()`, the clause `modals::workspace_modals_up`'s `perf_open` arm asks too
(`copy_table_view`'s rule): the preview is painted in the earlier DDL group, and a backdrop of
this one's over it would swallow Apply. Unlike *Copy to* it sets no `DdlPreview::then`, so after
Apply the view comes back with the list it last fetched, the dropped index still in it, until
Refresh. The modal is 860 wide (`modal_w`) at a fixed `PANEL_H` of 600 for
`server_vars_view::PANEL_H`'s reason; the *Statements | Indexes | Sizes* pills and, on Statements
only, *Total time | Mean time | Calls* (`pills`, over `widgets::pill_tab_style`) sit in a strip
under the title, and the footer is *Reading the server's statistics…* while loading, beside
Refresh and Close; Escape and the dismiss layer close it too. **Its words are the catalogue's**
(`perf-*`, `palette-performance` and its hint, and the gear's `tree-performance`); Close is the
shared `dialog-close`. English stays: the statements' text, the servers' names for indexes and
tables, and a fetch failure's message.
