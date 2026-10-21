# `app/history_store.rs`

`history_store.rs` — the query-history store: the persisted entry list, the run-id allocator,
and the five closures that write it. **Carved out of `app_view` because of an invariant, not a
line count**, which is the splitting procedure's own rule — and the invariant is that *every
mutation of this store is followed by a save, and which save is policy*. An ordinary
`persist::save_json` for a run recorded or finished; `persist::save_json_erasing` for the three
paths that **remove** — the panel's trash button, a row's Delete, and a connection being
deleted. That distinction is not cosmetic: an ordinary save copies the pre-change generation to
`history.json.bak`, so a confirm reading *"This can't be undone"* left every statement it named
on disk, and connection deletion's claim that the connection is not reconstructable from what is
left was false for the same reason. Before the cut the policy was five copies of the same
four-line block inside `app_view`, each choosing its own save; it is now stated once in a
private `save(entries, saving)`, which is also the only caller of the `FILE` const
(`"history.json"`) and the only place the `HistoryFile` wrapper is built.
`wire(connections, active_conn)` loads the file and hands back the `HistoryStore` — `entries`,
`record`, `finish`, `clear`, `remove`, `clear_conn` — with `RecordHistoryFn`, `FinishHistoryFn`
and `run_id_seed` (and its test) moved across from `main.rs`.
**The allocator is two functions, and the second one is a function so that a test can reach
it.** `run_id_seed` answers "the highest id on disk"; `allocate(next, n)` hands out the batch's
ids from the `Cell` seeded with it, each one past everything issued before. The `+ 1` that makes
an allocated id *past* the seed lived inside `record`'s `map`, where nothing could call it, so
`a_run_id_is_seeded_past_every_id_on_disk` asserted the seed alone while the word its own name
turns on went untested — and both mutations `run_id_seed`'s doc names survived it. Drop the
`+ 1` and every run of a session reuses the maximum id already on disk, so the first run to
land writes its timing and outcome onto a pre-existing entry: a row in the panel whose duration
and row count belong to a query somebody ran last week. `record` now takes the batch's ids up
front and zips them, and the test covers what its name says, including that a batch of nothing
burns no id its outcome would then look for.
**`record` and `finish` are deliberately not in `HistoryActions`**: they go to the run paths,
being how a query run reports itself rather than something the panel offers, so the store hands
them out separately. **`open_history` deliberately stayed in `app_view`** — it is the history
panel's click, but what it does is open a *tab* from a `HistoryEntry`, so it needs the
tab-id allocator and the placement closure and never touches this store; it is put into
`HistoryActions` beside `clear` and `remove` at the `Ui` literal. **An entry from another
connection switches to that connection first** — the panel's *All* tab lists
them, and the strip shows only the active connection's tabs, so a tab placed on another would
be active and invisible at once. The switch goes through `switch_conn_late`, the `LateAction`
the Open command takes for a file already open on another connection (`switch_conn` is defined
below both); an entry whose connection no longer exists opens nothing. And the connection-deletion
prune became a one-line `(history_clear_conn)(id)` call rather than moving, because the block it
sits in erases fourteen stores in a row under an invariant of its own, and each store being one
line is what keeps that readable.
`the_removal_paths_erase` is the module's reason for existing, as a gate: exactly three erasing
and two replacing saves inside `wire`, and `history.json` named once, as `FILE`. It scans its own
file through `source_gate::production_code` and carries that family's hazard — a gate in the same
file as its subject is its own first match — so its needles are assembled from fragments and its
counts are exact rather than floors. **Those totals are a floor now rather than the whole
check**, because a total holds just as well when two verbs are *swapped*: give `remove` the
`Replacing` and `record` the `Erasing` and every count is identical, while the deleted run
survives in `history.json.bak` under a modal saying it cannot be undone. The gate cuts `wire` at
its five `let` bindings with `source_gate::let_regions` and asserts per closure that the right
verb appears exactly once and the wrong one not at all; the totals stay, since what *they* catch
is a closure deleted rather than mis-saved. It was watched failing on exactly that swap. **The other half of it is in `main.rs`, and the move is what
found it**: `app_tests::deleting_a_connection_erases_every_store_it_was_keyed_into` counts
erasing saves in the delete closure with a floor of nine, the move took one of them out of that
file, and it read eight and failed. The repair was not to lower the floor — that is what stops a
different store's erase going missing — but to teach it that a **delegated** erase still counts:
it also counts a listed `delegated` needle (`"(history_clear_conn)("`, joined by
`"(snippet_clear_conn)("` when the snippet store followed), and its comment says to
add a line there *and* a gate in the store's own module, never only the line.
**A store it could not see at all is the sharper version of the same failure**, and it is the
limit of every census of this shape: the gate counts `Saving::Erasing`/`save_json_erasing(` inside the
closure's text and bans `Saving::Replacing` there, so a store pruned in the closure and saved
somewhere else entirely is neither counted nor banned. The Find-Anywhere search history was
exactly that — pruned in the delete closure, persisted by a `create_effect` ~8,300 lines away
calling `persist::save_json` — so the gate was green over a block whose own comment reads
*"every save below is erasing"* while that store's save was `Replacing`, copying the pre-prune
file to `search_history.json.bak` under a confirm reading *"It can't be undone"*. It has an
explicit saver now and the closure calls it with `Saving::Erasing` right after the prune, the
floor going 9 → 10 and watched failing at the old state.
**`ui/stored.rs`'s `Stored` is the same blind spot reached from the other side.** The five
stores it wraps save inside their own writers, so there is no `Saving::` text in the closure for
either half of the gate to see — and `Stored::update` saves `Replacing` just as invisibly as
`erase` saves `Erasing`. So the gate counts `<store>.erase(` for `db_colors`, `table_colors`,
`db_favorites`, `formats` and `accepted` toward the same count, and **bans `<store>.update(` for them in the
closure by name**, since the text ban on `Saving::Replacing` cannot. That ban was watched
failing against a `db_colors.update(` prune before the prune was switched to `erase`.
