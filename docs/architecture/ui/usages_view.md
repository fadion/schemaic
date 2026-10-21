# `ui/usages_view.rs`

`usages_view.rs` — the SQL editor's *Find usages* list (`usages_overlay`, raised by
`OverlayUi::usages` with a `UsagesRequest`, which `lib.rs` re-exports for the app): a modal
titled *N usages of name* (*usage* for one), whose rows are a line number and that line's text
in the mono family, ellipsised rather than wrapped, in a scroll capped at 360 scaled px, over a
Close button. The rows are `intel::usage_lines` over `intel::usages_at`'s answer (under
`intel.rs`), so it lists exactly the places the matching rename would write, and it knows no
buffer: `UsagesRequest::jump` is the editor's own, as `AliasRenameRequest::apply` is. Clicking a
row closes the list and then jumps; so does Enter or Space on the row under the cursor, the rows
being one Tab stop (`widgets::pick_group` at `NAV_TAB`, ahead of ✕ and Close) that Up/Down move
through — they were pointer-only, so the keyboard reached ✕ and Close and nothing the modal is
for. Close, Escape, ✕ and **a click on the backdrop** close it —
unlike *Rename alias*, it has a click-away. However it closes, it calls `UsagesRequest::back`,
for the reason `alias_rename.rs` gives. As the tuple element's third member it is in
`ddl_modals_up` (its `usages` arm) and on `PAINTS_A_BACKDROP`, for the reason the snippet
editor's entry gives.
**Its words are the catalogue's** (`usages-*`) — the title, `usages-title`, is
one message with a plural on `$count` and the name as `$name`, where it used to splice *usage*
or *usages* into an English sentence; Close is the shared `dialog-close`.
`the_title_reads_as_it_did_in_english` pins both numbers. The rows' line numbers and statement
text are the buffer's, and stay as they are.
