# `ui/server_vars_view.rs`

`server_vars_view.rs` — the **Server variables** viewer (`server_vars_overlay`), over
`core::server_vars`: the connection's settings as the engine reports them, in a read-only list
with a search box. **Read-only by design, and nothing here writes**: a `SET GLOBAL` or an
`ALTER SYSTEM` is a statement the user can type in the editor, through the write guard, and a
value field here would be a second path to the server with no guard on it. Server-level like
the Users browser beside it, so the modal takes no object. Mounted in the workspace group
directly after `users_overlay`, counted by `workspace_modals_up` whenever it is open (it raises
nothing, so the browser's preview clause has no counterpart here), and on
`modal_backdrop_gate::PAINTS_A_BACKDROP`.
**Three entry points, one door.** `GearKind::ServerVariables` in the SCHEMA gear and
`BlankKind::ServerVariables` in the tree's blank-space menu, each directly after `Users and
privileges` — skeleton group 2, a read of the server — **present** wherever a connection exists
and `supports_server_vars` says so (every engine today), and dimmed on a **down** connection but
not on a read-only one, the browser's gating for the browser's reason. The third is the
palette's *Server Variables*, an `instant` command **omitted** when there is no saved
connection, because a row that closes the palette and does nothing reads as broken; it asks no
capability and is not dimmed on a down connection, leaving both to the fetch. All three
call `open_for_active(conn, overlay)`, which resolves the active connection's name and dialect
(a no-op with none) and calls `open_for_server(overlay, conn_id, name, dialect)`; that resets
the kind to Variables, the state to `Loading` and the filter on the way in rather than on
close — `users_view::open_for_server`'s rule, so a second opening cannot flash the previous
server's list — and sets the `VarsTarget`, captured at open like `UsersTarget` so the list
describes the server it was opened on whatever the switcher does next. `VarsCtx` holds
`OverlayUi` and the one fetch, `SchemaActions::server_vars`, and is built at its call site in
`modals.rs`.
**The fetch effect keys on `(target, kind)` and lives above the `dyn_container`**, the
browser's rule: an effect created inside the child runs again on every rebuild of it. It
compares against its previous value because `create_effect` has no equality check of its own,
and switching *Variables | Status* is a fetch it makes. The app's `server_vars_fetch` (beside
`principals` in `main.rs`) asks `supports_server_vars` and, for Status, `supports_status`
**before** the round trip and reports `VarsState::Unsupported` — distinct from an empty
`Loaded`, which would read as "no settings" — then claims `overlay.server_vars_generation`
and checks it on landing alongside the target and the kind, `users_generation`'s guard: Variables
→ Status → Variables on a slow server is three fetches on one target, and only the last may
write. `VarsState::Loaded` holds an `Rc<Vec<ServerVar>>` because the list is rebuilt per
keystroke.
The modal is titled *Server variables — <connection name>*; the *Variables | Status* switch
(the table designer's pill tabs, `widgets::pill_tab_style`, `scaled(4)` apart) is built only
where `supports_status`, then the search `edit_field` (clearable, ring stop 10), then the scrolled list — name in mono, the value
beside it, the engine's description dim beneath where there is one. The list is keyed on
`(state, filter)` and draws `server_vars::render_slice`'s first `RENDER_CAP` rows with an
*N more settings match — narrow the search* line under them; the other states are *Loading…*,
the unsupported sentence, the failure in the error colour, and *No matching settings.* The
footer counts *N settings*, or *x of N settings* while the search is narrowing them, beside
Refresh and Close; Escape and the dismiss layer close it too.
**Its words are the catalogue's** (`server-vars-*`) — the title
(`$connection`), the search placeholder (through `FieldCfg::placeholder_of`), the *Variables |
Status* pills (`kind_switch`'s `tab` takes a `String`), the loading, unsupported and no-match
notes, the past-the-cap line `server-vars-more` (a plural on `$count`, which replaced
`text::plural` — the file no longer imports it), the footer's `server-vars-count` (a plural)
and `server-vars-count-filtered` (`$shown` of `$count`), and Refresh; Close is the shared
`dialog-close`. `the_counted_lines_read_as_they_did_in_english` pins the title and the counted
lines in both numbers. English stays: the variables' names, values and descriptions — the
server's own text — and a fetch failure's message.
