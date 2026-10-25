# Schemaic — architecture

A native SQL editor (Rust + [Floem](https://github.com/lapce/floem) 0.2.0), MySQL/MariaDB-first,
Zed-inspired, aiming to replace DataGrip. PostgreSQL and SQLite are wired too; SQLite is
read/write and edits **tables** (through the twelve-step rebuild), **views** and **triggers**, but
has no manual-transaction mode — see `db::session`'s `Session::open` for what that one is a
statement about. All three engines now edit all three of those objects, and they get there
differently, so ask the *narrow* capability (`ddl::supports_or_replace_view`,
`ddl::supports_view_rename`) rather than the engine. **Microsoft SQL Server is a fourth, and a
full one, as MySQL/MariaDB and PostgreSQL are**: it connects, reads, validates, introspects, runs
scripts, writes the grid's edits back, imports a file into a table, designs tables, edits views,
triggers and stored routines, drops a table, view or routine, holds a Manual tab's transaction,
shows a query plan, dumps a database to a `.sql` file, creates and drops a database or a schema
inside one, and browses and administers its logins, database users and their grants. Its
sequences, alias types, synonyms and XML schema collections are browsed in the schema tree,
created and edited in the object editor, dropped from their rows and compared. What it does not do
yet is named rather than left to a label, under `db::mssql`; the last index kinds on that list, a
selective XML index and an ordered columnstore one, are read, restated and authored now.

This is the project's reference document: the crate/module map, the architecture invariants, the
UI conventions, and the Floem hazards each subsystem is built on. `AGENTS.md` at the repo root
holds the *working* rules — how to build, test and commit — and points here for everything else.
Keep the two disjoint: an instruction to the person or agent doing the work belongs there, a fact
about the system belongs here.

**This file is the index; the document itself is in `docs/architecture/`**, one file per module
entry and one per cross-cutting section. **Open only the file you need**, and prefer not to open
it at all: `scout` (in `.claude/agents/`) answers "where/how" questions against these files *and*
the code and reports back a conclusion rather than a transcript, and `arch-scribe` makes the edits
a finished change requires. Several files are still large — `invariants.md`, `floem-gotchas.md`,
`data-grid.md`, `core/ddl.md`, `core/intel.md` — so grep them and read the range, as before.

**A citation of `docs/architecture.md` from before the split still resolves.** A comment that
names a section — *Architecture invariants*, *Floem 0.2 gotchas*, *Data grid*, *The string
catalogue* — means the file of that name below, or the bullet of that name inside one; a comment
that points at a module's entry means `docs/architecture/<crate>/<module>.md`. Grep
`docs/architecture/` for the name.

## Cross-cutting

| File | Section | Covers |
| --- | --- | --- |
| [`invariants.md`](architecture/invariants.md) | Architecture invariants (don't regress these) | The rules re-introducing which is a regression, each with the bug behind it |
| [`ui-conventions.md`](architecture/ui-conventions.md) | UI conventions | Cursors, labels, colours, theming, the string catalogue |
| [`floem-gotchas.md`](architecture/floem-gotchas.md) | Floem 0.2 gotchas (learned the hard way) | The framework hazards: focus, Tab, scroll, overlays, transitions |
| [`popup-menus.md`](architecture/popup-menus.md) | Popup menus (`menu_panel`) | `menu_panel`, submenus, dismissal, edge-flipping |
| [`results-strip.md`](architecture/results-strip.md) | The results strip (and keeping a result) | `ResultPanel`s, the strip that lists them, and keeping a result |
| [`data-grid.md`](architecture/data-grid.md) | Data grid (results grid) | The results grid, top to bottom |

## Crates

One file per module entry, under `docs/architecture/<crate>/`, named after the module it describes
and listed here in the order they are best read in. Each crate's `overview.md` is what the crate's
entry said before its modules: what the crate is for, and for `schemaic-db` the dispatcher and the
MySQL and PostgreSQL legs, whose modules have no entries of their own. Three entries cover more
than one module and are named after the first: `ui/theme.md` (with `themes.rs`, `icons.rs`,
`fonts.rs`, `sql_highlight.rs`), `ui/table_designer.md` (with `ddl_preview.rs`), and
`core/ai-plumbing.md` and `core/ui-state-models.md`, which group small modules under one heading.
`schemaic-db`'s files are cut from one entry at its seams, so `db/connect.md`, `db/import.md` and
`db/live-tests.md` are named for what they cover rather than for a module.

- **`schemaic-core`** — [overview](architecture/core/overview.md) · [model](architecture/core/model.md) · [access](architecture/core/access.md) · [aggregate](architecture/core/aggregate.md) · [chart](architecture/core/chart.md) · [chart_draw](architecture/core/chart_draw.md) · [result_diff](architecture/core/result_diff.md) · [sql](architecture/core/sql.md) · [intel](architecture/core/intel.md) · [pg_builtins](architecture/core/pg_builtins.md) · [mssql_builtins](architecture/core/mssql_builtins.md) · [quickinfo](architecture/core/quickinfo.md) · [filter](architecture/core/filter.md) · [skeleton](architecture/core/skeleton.md) · [edit](architecture/core/edit.md) · [blob](architecture/core/blob.md) · [export](architecture/core/export.md) · [import](architecture/core/import.md) · [dump](architecture/core/dump.md) · [ddl](architecture/core/ddl.md) · [compare](architecture/core/compare.md) · [copy_table](architecture/core/copy_table.md) · [erd](architecture/core/erd.md) · [erd_export](architecture/core/erd_export.md) · [monitor](architecture/core/monitor.md) · [activity](architecture/core/activity.md) · [users](architecture/core/users.md) · [server_vars](architecture/core/server_vars.md) · [scram](architecture/core/scram.md) · [diff](architecture/core/diff.md) · [snippet](architecture/core/snippet.md) · [tab_stops](architecture/core/tab_stops.md) · [settings_bundle](architecture/core/settings_bundle.md) · [locale](architecture/core/locale.md) · [history](architecture/core/history.md) · [write_log](architecture/core/write_log.md) · [sticky_header](architecture/core/sticky_header.md) · [health](architecture/core/health.md) · [window_chrome](architecture/core/window_chrome.md) · [window_place](architecture/core/window_place.md) · [tx](architecture/core/tx.md) · [typename](architecture/core/typename.md) · [format](architecture/core/format.md) · [conn_import](architecture/core/conn_import.md) · [connection](architecture/core/connection.md) · [ssh_config](architecture/core/ssh_config.md) · [init_sql](architecture/core/init_sql.md) · [schema](architecture/core/schema.md) · [persist](architecture/core/persist.md) · [secrets](architecture/core/secrets.md) · [rowjson](architecture/core/rowjson.md) · [jsontree](architecture/core/jsontree.md) · [celledit](architecture/core/celledit.md) · [date](architecture/core/date.md) · [plan](architecture/core/plan.md) · [ai-plumbing](architecture/core/ai-plumbing.md) · [text_ops](architecture/core/text_ops.md) · [body_search](architecture/core/body_search.md) · [used_by](architecture/core/used_by.md) · [sqlfmt](architecture/core/sqlfmt.md) · [pairs](architecture/core/pairs.md) · [params](architecture/core/params.md) · [script](architecture/core/script.md) · [sqlfile](architecture/core/sqlfile.md) · [launch](architecture/core/launch.md) · [entra](architecture/core/entra.md) · [cloud_iam](architecture/core/cloud_iam.md) · [cli_install](architecture/core/cli_install.md) · [ui-state-models](architecture/core/ui-state-models.md)
- **`schemaic-conn`** — [overview](architecture/conn/overview.md) · [secrets](architecture/conn/secrets.md)
- **`schemaic-db`** — [overview](architecture/db/overview.md) · [sqlite](architecture/db/sqlite.md) · [mssql](architecture/db/mssql.md) · [entra](architecture/db/entra.md) · [cloud_iam](architecture/db/cloud_iam.md) · [connect](architecture/db/connect.md) · [tls](architecture/db/tls.md) · [import](architecture/db/import.md) · [live-tests](architecture/db/live-tests.md)
- **`schemaic-ai`** — [overview](architecture/ai/overview.md) · [harness](architecture/ai/harness.md) · [stream](architecture/ai/stream.md)
- **`schemaic-term`** — [overview](architecture/term/overview.md)
- **`schemaic-ui`** — [overview](architecture/ui/overview.md) · [consts](architecture/ui/consts.md) · [widgets](architecture/ui/widgets.md) · [tooltip](architecture/ui/tooltip.md) · [modals](architecture/ui/modals.md) · [markdown](architecture/ui/markdown.md) · [i18n](architecture/ui/i18n.md) · [settings](architecture/ui/settings.md) · [shortcuts](architecture/ui/shortcuts.md) · [connection_form](architecture/ui/connection_form.md) · [connection_import](architecture/ui/connection_import.md) · [dividers](architecture/ui/dividers.md) · [history_panel](architecture/ui/history_panel.md) · [snippet_edit](architecture/ui/snippet_edit.md) · [alias_rename](architecture/ui/alias_rename.md) · [usages_view](architecture/ui/usages_view.md) · [source_gate](architecture/ui/source_gate.md) · [stored](architecture/ui/stored.md) · [snippet_panel](architecture/ui/snippet_panel.md) · [activity_panel](architecture/ui/activity_panel.md) · [plan_view](architecture/ui/plan_view.md) · [properties](architecture/ui/properties.md) · [used_by_view](architecture/ui/used_by_view.md) · [chart_view](architecture/ui/chart_view.md) · [result_diff_view](architecture/ui/result_diff_view.md) · [users_view](architecture/ui/users_view.md) · [server_vars_view](architecture/ui/server_vars_view.md) · [import_view](architecture/ui/import_view.md) · [script_view](architecture/ui/script_view.md) · [dump_view](architecture/ui/dump_view.md) · [table_designer](architecture/ui/table_designer.md) · [view_editor](architecture/ui/view_editor.md) · [window_chrome](architecture/ui/window_chrome.md) · [trigger_editor](architecture/ui/trigger_editor.md) · [routine_editor](architecture/ui/routine_editor.md) · [event_editor](architecture/ui/event_editor.md) · [object_editor](architecture/ui/object_editor.md) · [database_editor](architecture/ui/database_editor.md) · [account_editor](architecture/ui/account_editor.md) · [ai_panel](architecture/ui/ai_panel.md) · [overlays](architecture/ui/overlays.md) · [schema_tree](architecture/ui/schema_tree.md) · [tabs](architecture/ui/tabs.md) · [cell_editors](architecture/ui/cell_editors.md) · [grid](architecture/ui/grid.md) · [inline_diff](architecture/ui/inline_diff.md) · [erd_view](architecture/ui/erd_view.md) · [erd_raster](architecture/ui/erd_raster.md) · [monitor_view](architecture/ui/monitor_view.md) · [blob_view](architecture/ui/blob_view.md) · [compare_view](architecture/ui/compare_view.md) · [copy_table_view](architecture/ui/copy_table_view.md) · [theme](architecture/ui/theme.md) · [contrast](architecture/ui/contrast.md) · [lib](architecture/ui/lib.md)
- **`schemaic-app`** — [overview](architecture/app/overview.md) · [agent_cli](architecture/app/agent_cli.md) · [antigravity](architecture/app/antigravity.md) · [liveness](architecture/app/liveness.md) · [opencode](architecture/app/opencode.md) · [copilot](architecture/app/copilot.md) · [cursor](architecture/app/cursor.md) · [conn_sources](architecture/app/conn_sources.md) · [script](architecture/app/script.md) · [history_store](architecture/app/history_store.md) · [snippet_store](architecture/app/snippet_store.md) · [ui_stores](architecture/app/ui_stores.md) · [heap](architecture/app/heap.md) · [logging](architecture/app/logging.md) · [notify](architecture/app/notify.md) · [update](architecture/app/update.md) · [install_cli](architecture/app/install_cli.md)
- **`schemaic-cli`** — [overview](architecture/cli/overview.md) · [args](architecture/cli/args.md) · [select](architecture/cli/select.md) · [query](architecture/cli/query.md) · [catalog](architecture/cli/catalog.md) · [exec](architecture/cli/exec.md) · [dump](architecture/cli/dump.md) · [compare](architecture/cli/compare.md) · [deadline](architecture/cli/deadline.md) · [format](architecture/cli/format.md) · [run](architecture/cli/run.md) · [mcp](architecture/cli/mcp.md) · [schemaic-cli](architecture/cli/schemaic-cli.md)

## Keeping this true

**It is only worth this much if it stays true.** Silent drift from the code is the most damaging
kind of bug in a document everyone trusts — `core/tests/doc_coverage.rs` catches a module that was
never written down, and nothing catches a paragraph that has quietly become false. When a change
lands, route the write through `arch-scribe` rather than leaving it for afterwards. Each file states
the system as it is now: when a change alters a rule, rewrite the passage that states it, in place.

**A new module gets its own file**, `docs/architecture/<crate>/<module>.md`, headed with the
module's crate-qualified name (`` # `core/ddl.rs` ``), and a link in its crate's line above, placed
beside its nearest relative. A small module that belongs to a group may join that group's file
instead. `doc_coverage` reads this index and every file under `docs/architecture/`, so either
satisfies it; its second test fails on a file under `docs/architecture/` that this index does not
link, so the index cannot fall behind the directory. **When a file grows past a size that makes
it hard to read and has a seam**, split it there and add the link.

**A bare basename does not always answer that guard.** It matches on a left word boundary, because
a plain `contains` could not fail for twelve of the modules it governs — seven basenames are
suffixes of another module's (`date.rs` ⊂ `update.rs`, `import.rs` ⊂ `conn_import.rs`, and five
more), and the longer name is itself guaranteed to be written down, so deleting every mention of
`core/date.rs` left the guard green. And a basename that exists in **two** crates — `dump.rs`,
`format.rs`, `script.rs`, `secrets.rs`, `update.rs`, `window_chrome.rs` — must be named with its
crate, since `app/secrets.rs` answered for `core/secrets.rs` for free while the failure message
told the reader to add them per crate. Everywhere else a bare mention is still enough.
