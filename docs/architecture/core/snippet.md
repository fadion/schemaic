# `core/snippet.rs`

`snippet.rs` — the snippet library: named saved queries, persisted to `snippets.json`.
`applies` answers whether a snippet may be offered on a connection, `grouped` builds the
panel's headings (**narrowest bucket first** — this connection, this engine, everywhere — each
snippet under exactly one, empty buckets omitted; that last clause is load-bearing outside this
module, since it makes two connections with no connection-scoped snippet group *identically*,
which `two_connections_group_identically_when_neither_has_a_scoped_snippet` asserts as the
precondition behind the snippet panel's frozen-`active_conn` defect), `matches_query` is the panel's filter
(name/abbrev/body, the body whitespace-collapsed the way `history::matches_query` reads a
statement), `by_abbrev` is the completion trigger (whole-word, case-insensitive, **a snippet
the user wrote wins, then the narrowest scope**), `scope_options` is the scope picker's three
choices **in the bands' own order** — a test pins each choice to the heading a row moves to
when it is picked, so the picker and `grouped` cannot drift — plus
`next_id`/`touch`/`remove`/`clear_conn`.
**A pack of built-in snippets ships in the code**, not in anybody's `snippets.json`:
`builtins(dialect)` returns them per engine (`Source::Builtin`, `Scope::Dialect`), and
`library(user, dialect)` is the merged view — user snippets first, then the pack — that every
consumer must be handed, because `applies`/`grouped`/`by_abbrev` all answer about a list. They
are kept in code so a later release can fix one; a shipped snippet is not editable or
deletable (the panel offers Duplicate instead), which is exactly why the *source* has to be
`by_abbrev`'s first ranking key: every built-in is `Scope::Dialect`, so ranking scope first put
the pack above a user snippet moved to "All connections" — a snippet they wrote, losing its
abbrev to one they cannot delete or re-spell.
`clear_conn`/`count_conn` drop a deleted connection's `Scope::Conn` snippets, for the two
reasons `delete_conn_now` states about its thirteen siblings: a deleted connection should not be
reconstructable from what is left on disk, and connection ids are **recycled**, so a snippet
left behind is inherited by the next connection to take the freed id, under a heading reading
"THIS CONNECTION".
**Scope is `Global | Dialect | Conn`, not a `conn_id` like `history.rs` uses** — that is the
difference between a library and a log: a "running queries" snippet is wanted on every MySQL
connection, not on the one it was saved from. Nothing is capped or evicted, for the same
reason. A body's `:name` placeholders are not stored (they would go stale on the first edit)
and need no tab-stop syntax of their own — inserting the body hands them to `params`, and
**the placeholders are the tab stops** (`tab_stops.rs`, below): a second template mechanism
would have to be taught everything the first already knows, and a stop left unfilled is
still the parameter it was.
`Scope` is a **preserving** persisted enum: `Unknown(String)` keeps the text it didn't
recognise and the hand-written `Serialize` writes it back verbatim, the rule
`search_history::ObjectTag` states, because this file is rewritten whole on every change.
**`edits`/`SnippetEdits` is what the editor modal's Save writes**, and it is here rather than in
the dialog because the dialog compared the body *trimmed* and wrote it *untrimmed*. An edit
living entirely in the leading or trailing whitespace was therefore invisible to the guard:
deleting the two blank lines off the end of a body closed the modal as if it had saved, and
reopening it showed them still there. The two sibling guards four lines above have no such
asymmetry — name and abbrev are trimmed on both sides of the comparison *and* in the write —
which is exactly what made this look covered. `SnippetEdits`' three `Option`s are "did it
change, and with what", so the comparison and the payload are the same value by construction;
name and abbrev stay trimmed because they are looked up and matched on, the body is stored
verbatim because its whitespace is part of what gets inserted, and an emptied name is *no
change* rather than a rename to nothing. The decision could not be tested where it lived — the
closure needs a Floem scope — and
`a_body_edit_that_is_only_whitespace_is_still_a_change` reports `None` against the trimmed
comparison where `Some("SELECT 1")` is the edit the user made.
`next_id` is max-plus-one **over the user's snippets only** — it filters out anything at or
above `BUILTIN_ID_BASE`, so the pack's ids never raise the counter and a careless caller
passing the merged `library()` list gets the same answer as one passing the user's. It
therefore **reuses a deleted snippet's id**; that is safe only while no id outlives the file,
which is why snippet activations are not recorded in `search_history.json` — a test pins both
limits.
**Four decisions came down here out of closures in `app_view`, where nothing could call them.**
`snippet_text(sql, selection)` is *what* to save — the selection if there is one, else the whole
buffer — and `can_save(sql)` is whether the `+` is offered at all; they are one function apart
rather than two reads at a call site because a whitespace-only selection made them disagree, and
`+` was enabled for a click that saved nothing. What refuses a blank selection is
`text_ops::selected_text`, which answers `None` for a range that is empty, reversed,
whitespace-only or out of bounds — that last being the mirrored range having drifted a keystroke
out of step with the text, so a stale range saves the buffer rather than a slice of the wrong
statement. The call site used to re-filter the blank case on top of it with a
`.filter(|s| !s.is_empty())`; that was **dead code** — `selected_text` had already refused every
input it could have caught — which is not what the change was written believing, and running the
revert is what settled it. Removing it leaves one guard worth pointing a revert at, and
`snippet_save_agrees_with_the_button_that_offers_it` is the pin over the inputs that separate
the two. `is_builtin(id)` is the `>= BUILTIN_ID_BASE` test read as a
question — a shipped snippet has nothing to record and nothing to persist, so `touch` would find
nothing and the save behind it would rewrite the file for no change on every insert of one. And
`duplicate(src, new_id)` is the copy **Duplicate** makes, with the five things it does not carry:
`source` becomes `User` even from a built-in (that is what Duplicate is *for* on a shipped
snippet that cannot be edited in place), `abbrev` is dropped because two snippets answering to
one spelling is a coin toss, `last_used` is `None`, the name gains ` copy`, and `scope` is
inherited because the copy is for the same place as the original.
