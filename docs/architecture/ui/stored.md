# `ui/stored.rs`

`stored.rs` — `Stored<T>`, **a persisted store the UI can read freely and write only by
saving**. `formats`, `db_colors`, `table_colors` and `db_favorites` used to sit on `Ui` as public
`RwSignal`s beside three public save closures (`save_formats`, `save_db_colors`,
`save_db_favorites`), so writing a signal and forgetting the save compiled — and looked right
until a restart: the swatch painted, the star turned gold, and the choice was gone the next
launch. The only thing between that and a shipped bug was `persisted_store_gate`, a needle (see
`source_gate.rs` above). `Stored` makes the pairing unspellable instead: its signal and its
saver are private, and the only writers are `update`, which saves `Saving::Replacing`, and
`erase`, which saves `Saving::Erasing`. *Which* save is still the caller's to say — an upsert
keeps the previous file as `.bak`, a deletion must not — but *whether* to save no longer is.
Both mutate first and save second, and the saver's `Rc` is cloned out of its signal before it
is called rather than invoked inside that borrow, because a saver reads the store's own signal
(and, for the colour pair, its sibling's) and nothing should be held while it runs. The readers
are `get`, `get_untracked`, `with`, `with_untracked` and `read_only() -> ReadSignal<T>`, and the
last is what every consumer that only reads now takes — `db_color_dot`, `table_color_dot`,
`favorite_star`, the ER overlay, the history panel, the tab strip, `visible_nav_rows` and the
schema tree's node-context structs — so the writable handle reaches only the three places that
write: the schema tree's right-click menu in `overlays.rs` (four colour entries and one
favourite), the grid's `set_format`, through `GridState::fmt_rules`, and
`completion::accept_completion`, through `Completion::accepted` (under `completion.rs`).
**The saver sits in a signal of its own (`RwSignal<Rc<dyn Fn(Saving)>>`) so that `Stored` is
`Copy`**, like the `RwSignal` it replaced: `GridState` stays `Copy` and every closure that
captured the old field keeps its shape, and the grid's `RwSignal<Option<SaveFormats>>` wrapper
around the old save closure is gone with the `SaveFormats` alias. **`Stored::new` takes the
signal, not a value**, because two stores share one file — `db_colors` and `table_colors` are
both `db_colors.json` — and the shared saver has to read both signals before either store
exists. The trade is that whoever builds a `Stored` *could* keep the `RwSignal` and write around
it; the one place that does is `app/ui_stores.rs`'s `wire`, and only for as long as it takes to
build the savers. Seven tests pin the type, the load-bearing one being
`the_save_runs_after_the_change_not_before` — a saver that ran first would write the previous
generation and lose the choice just made — beside one save per write and of its own kind, reads
saving nothing, a copy being the same store, and two stores sharing a saver each writing the
whole file. **What it cannot cover is which writer a caller picks**: `update` in the
connection-delete prune compiles and saves `Replacing` with no `Saving::Replacing` in the text,
so `app/main.rs`'s census bans `<store>.update(` there by name (under `history_store.rs`).
