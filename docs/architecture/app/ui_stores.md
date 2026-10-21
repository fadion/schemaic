# `app/ui_stores.rs`

`ui_stores.rs` — the small stores **the UI owns and the app only persists**: per-column
display formatters (`format.json`), identity colours for databases and tables (`db_colors.json`),
favourited databases (`favorites.json`) and the completion items each connection has accepted
(`accepted.json`, the fourth and latest). **The third cut of the `app_view` split, and
deliberately *not* the sibling of the two above — the difference is what this entry is for.**
Those two own their mutators, so each can hold the rule *every mutation is followed by the right
kind of save* and gate it exactly. These cannot: every mutation is in `schemaic-ui` — the
grid's "Format as" menu, the schema tree's Colour swatches and Favourite/Unfavourite, the
completion popup's accept — so the rule
lives in the type they are handed as instead, `ui/stored.rs`'s `Stored`, whose only writers
save. **This module is the one place a writable signal for them exists**, and only for as long
as it takes to build the savers that read it: `wire()` makes each `RwSignal` and its saver and
returns `Stored::new(signal, saver)` for each — five handles over four files, the colour pair
sharing `save_db_colors` — and `UiStores` holds those five and nothing else. What the module
owns beyond that is the four file names, once each as consts, and one `saver(file, build)`
helper so a fifth store cannot arrive with a shape of its own. `build` is re-run
on every save rather than captured once, because the UI writes these signals between saves and a
captured value would persist whatever the store held when `wire` ran. Its own gate,
`each_store_is_named_once`, is correspondingly narrow — one name and one `saver` call per store —
and its doc comment says why that is enough now: the rule those stores actually need is not a
test here at all but `Stored`, whose own tests pin it. A reader arriving from `history_store.rs`
will otherwise look for the gate those two have. **Until `Stored`, the signals went raw into
the `Ui` bundle beside three public `save_*` closures, and the whole guarantee was
`persisted_store_gate`**, a needle in `schemaic-ui` (under `source_gate.rs`, where its deletion
is recorded): `format.json`'s writer was exempted from it on a stated reason that turned out to
be false of `grid.rs`, so for as long as that stood, deleting the formatter's save left the
suite green while this module's prose pointed at a gate covering two of its three stores.
**The one invariant left on this side is that `db_colors` and `table_colors` are two signals
over one file**, so writing either half has to write both or the other is lost; `saver`'s closure
builds the whole `DbColorsFile` on every save, and the struct literal is what makes that
unforgettable — a save that wrote one half would not compile. Each save still takes its
`persist::Saving`, because each has a caller that is a **deletion**: the connection-deletion
prune must erase, or the deleted connection's databases, tables and columns stay readable in
`<store>.json.bak` under a confirm saying they cannot be recovered. It does so with
`Stored::erase` on all five, so the colour pair's file is written twice — the second write the
one with both halves pruned, and neither leaving a `.bak`. That dispatch is
`persist::write_json_store`'s, which is why — unlike the other two stores — this module holds no
`match` of its own. It made `serde` a direct dependency of `schemaic-app`, which it already was
transitively through `serde_json`: one line in `Cargo.toml` and nothing new to compile.
