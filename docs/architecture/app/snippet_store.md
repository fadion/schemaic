# `app/snippet_store.rs`

`snippet_store.rs` — the snippet library's store: the persisted **user** list and the ten
closures that write it. **The matched pair to `history_store.rs`, and the pairing is the
point** — two modules of the same shape rather than one "stores" grab-bag, because what
justifies each of them is the same invariant: every mutation of the store is followed by a save,
and which save is policy. `persist::save_json_erasing` from the two paths that **remove** — a
snippet deleted from its row's menu, and a deleted connection's connection-scoped snippets — and
an ordinary `persist::save_json` from the eight edits (`record_use`, `create`, `rename`,
`set_abbrev`, `set_body`, `set_scope`, `duplicate_from`, and `import`, a settings bundle's
snippets merged through `settings_bundle::merge_snippets` in one update and saved once, and
only when something was added). A snippet is the user's own SQL written
against a named server, so an ordinary save on the delete copies the pre-delete file — body and
all — to `snippets.json.bak` at the moment the modal says *"This can't be undone"*; on a library
nobody edits again, that copy is forever. The choice is stated once in a private
`save(snippets, saving)`, which is also the only caller of the `FILE` const (`"snippets.json"`)
and the only place `SnippetsFile` is built; `wire()` loads the file and hands back the
`SnippetStore`.
**The seam is the store against the editor, not the panel against the app**, which is what made
this cut non-obvious: what stays in `app_view` is everything that reads a *tab* or raises a
*modal*, so four closures came apart in half at that line rather than moving whole.
`insert_snippet` writes the active tab's `insert_req` — the mounted editor owns the document, so
writing `query` behind it loses the insertion's undo step — and then calls `record_use`.
`save_snippet_current` reads the selection through `snippet::snippet_text` and names the snippet
from the tab's title before calling `create(name, body, conn_id)`, which is why the store takes
both rather than reaching for them. `remove_snippet` raises the confirm and calls `remove(id)`
only on yes: **`SnippetStore::remove` carries no confirm of its own**, asking being the UI's job
and erasing the store's — the same split `delete_conn`/`delete_conn_now` already has in that
file. `duplicate_snippet` looks its source up in the **merged** library memo, Duplicate existing
mainly to get an editable copy of a built-in and a built-in not being in the user's list to be
found, then hands it to `duplicate_from`. `snippet_library` and `can_save_snippet` stay as memos
over signals this store does not own (`conn_dialect_memo`, the tab strip), and `active_tab`,
`conn_dialect_memo` and `editor_snippet_text` stay because they are shared — `active_tab` has
other readers in `app_view` and `conn_dialect_memo` goes into the `Ui` literal. **`snippets` is
the user's list alone, never the merged library**, which is what keeps the built-in pack in code
where a later release can fix one instead of copied into everybody's `snippets.json`.
`the_remove_path_erases` is the module's gate, deliberately spelled as the twin of
`history_store::the_removal_paths_erase` so the pair reads as one rule in two places: exactly
two erasing saves and eight replacing ones inside `wire`, and `snippets.json` named once, as
`FILE`. It reads its own source through `source_gate::production_code` and carries that family's
hazard — a gate in the same file as its subject is its own first match — so its needles are
assembled from fragments and its counts are **exact rather than floors**, which is not pedantry:
deleting `rename`'s save failed it at 6 ≠ 7, and what that spells is a writer silently no longer
persisting, which a floor would have passed. **What a total cannot catch is a *swap*** — give
`remove` the `Replacing` and one edit the `Erasing` and every count is unchanged, while a
deleted snippet's body survives in `snippets.json.bak` for as long as the library is never
edited again — so the twin pairing holds here too: `source_gate::let_regions` cuts `wire` at its
ten `let` bindings and each closure is asserted against the one verb it may use, with the
totals left standing underneath to catch a closure deleted rather than mis-saved.
The other half is in `main.rs`, where
`deleting_a_connection_erases_every_store_it_was_keyed_into` gained its **second** delegated
needle (`"(snippet_clear_conn)("` beside `"(history_clear_conn)("`), exactly as its own comment
instructs. `clear_conn` is the prune that block's prose calls *the twelfth store, and the one
that was missed*, and that reasoning now also sits on `SnippetStore::clear_conn`'s doc.
