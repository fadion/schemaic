# `ui/used_by_view.rs`

`used_by_view.rs` — the **Used by** modal (`used_by_overlay`), over `core::used_by`, opened by
setting `overlay.used_by` to a `UsedByTarget { database, schema, name, column, routine }`
(re-exported as `schemaic_ui::UsedByTarget`; the app creates the signal beside `properties`)
from the schema tree's menu on a table or view, a column, or a routine. Properties' shape and
read-only like it: `modal_title_owned` *"Used by — x"*, a scrolling body, a footer of **Copy**
(the list as Markdown) and **Close**, Escape and the backdrop closing it. The body groups the
dependents under a counted heading per kind — `widgets::fact_section_owned`, *"Views (3)"* —
each row the name in mono and its detail beside it, a `goes_with_it` one marked *"· dropped
with it"*, since the list is read before a drop; and under every answer one faint line saying
what the scan does not see: SQL built at run time, and other databases.
**No fetch and no loading state** — it opens with its answer. `dependents_of(schema, conn,
target)`, which the table Drop's preview warning calls too, reads the database's schema through
`table_designer::loaded_schema` and returns `None` while that is not loaded (the same funnel,
so mid-refresh too), and the modal **says so** — *"expand it in the tree, then ask again"* —
rather than *"Nothing in db uses it."*, which is the separate answer for an empty list. The Drop
has no such note to show: with no schema loaded its preview simply carries no *Used by*
sentence, and neither does a rename's or column edit's (`ddl_preview::with_used_by`). The dialect is `ConnUi::dialect`, the active connection's, which is right because
every database the tree shows is that connection's.
Mounted in the workspace group directly after `properties_overlay`, and counted by
`workspace_modals_up` whenever it is open — it raises nothing, so it is a plain term like
`vars_open`.
**Its words are the catalogue's** (`used-by-*`) — the title (`$object`, also
the Copy text's first line), Copy, the not-loaded and nothing-in notes (`$database`), the
limits line, *· dropped with it* (`$detail`) and the Copy text's *Nothing uses it.*; Close is
the shared `dialog-close`. The kind words are core's, drawn here by the typed-value rule:
`dependent_heading(DependentKind, n)` is one whole message per kind with a plural on `$count`,
replacing `kind.noun(n)` capitalised with the count spliced on (the `capitalised` helper is
gone), and `dependent_noun` the Copy text's singular in place of `kind.noun(1)`.
`english_words_every_dependent_kind_as_core_does` holds both to core's composition for every
kind in both numbers, and `the_composed_lines_read_as_they_did_in_english` pins the rest. The
Copy text (`as_markdown`) is translated with the screen — it is what the user copies of what
they read, not a file format — and keeps its Markdown punctuation. A dependent's `detail`
(core's prose, phase 4) and the names stay English.
