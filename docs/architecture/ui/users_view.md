# `ui/users_view.rs`

`users_view.rs` — the **Users and privileges** browser (`users_overlay`), over `core::users`.
It lists the server's accounts, shows one account's privileges, and is where the five write
actions are raised from — the forms themselves are `account_editor.rs`. Mounted in the
workspace group beside `properties_overlay`/`erd_overlay`/`monitor_overlay` and counted by
`workspace_modals_up`.
**Everything it reads out of the root bundle is one struct, `UsersCtx`, and that struct holds no
`Ui`.** The modal root, both panes and the footer take it: `ConnUi`, `DdlUi` and `OverlayUi` —
three `Copy` bundles — plus the two fetch closures (`principals`, `grants`) as `Rc`s, which is
the whole of what the browser reaches for. It is built by naming those reads at its one call
site, `UsersCtx::new(ui.conn, ui.ddl, ui.overlay, &ui.schema_actions)` in `modals.rs`, rather
than by a constructor taking `&Ui`; that is what took this file off `whole_ui_gate`'s list
entirely, and `lib.rs` has why a context struct holding the root bundle would not have.
`open_for_server` is deliberately *not* on it — see below.
**It renders nothing while one of its own forms or the DDL preview is up.** Its `dyn_container`
is keyed on `(target, hidden, target_read_only)`, where `hidden` reads
`ddl.account`/`ddl.grant`/`ddl.preview`,
and the outer fill style asks the same question. That is the pairing every schema editor already
has with the preview one level up, and here it is load-bearing rather than tidy: the two account
forms live in the modal layer's **DDL group**, which is painted *before* the workspace group, so
both opened *behind* the browser that raised them. `modals::workspace_modals_up` asks the
identical three signals, because a wrapper that still filled the layer would be a transparent
full-window box sitting on top of the form and swallowing every click meant for it. Cancel over
there returns here with the list intact.
**The live read-only flag is the third term of that key, because `write_gate` reads it** and the
gate is computed once per container build. Nothing re-ran it when the flag moved, so flipping the
connection to read-only from the status bar left **Drop** lit, the "This connection is
read-only." note absent, and Apply enabled on a preview that then silently did nothing.
`target_read_only` is the *tracked* reader of that flag and sits deliberately beside
`table_designer::edit_ctx`'s untracked one: `get_untracked` is the right reading at a click and
the wrong one as a container key, since a browser keyed on it has to rebuild when the answer
changes — which is the whole point of the question. Both readings are wanted here, so there are
two: `target_read_only` for the key and `launch_read_only(conns, conn_id)` for a click. The
second is `pub(crate)`, because `account_editor`'s three doors asked `edit_ctx`'s `read_only` —
the *switcher's* — while the buttons that launch them were lit from the target's, and the
repair for that is this browser's own answer given to the doors rather than a fourth spelling of
it next door. Drop was already asking it; see `account_editor.rs`'s entry for what the other
three did instead.
**That third term is load-bearing in a way the function's own doc obscures, so a test pins it.**
`write_gate` calls `target_read_only` again *inside* the builder, and that function's doc says it
reads tracked — true of the function, which uses `.with` rather than `.with_untracked`, and not
true of that caller, since a `dyn_container` builder is not a tracking scope. The gate inside
reads the right value only because the key's own term already forced the rebuild, so the doc
reads like coverage for a read that has none, and dropping the term on the strength of it is a
one-line edit that compiles, looks like a simplification and reproduces Server Activity's bug
exactly. `the_browser_key_carries_the_read_only_flag` is a source gate over this module's
production text (through `source_gate::production_code`, not a cut at the first `#[cfg(test)]`):
it finds the container by `target.get()` in its key and fails unless `target_read_only(` appears
in the ten lines from there — a window deliberately short of the builder's own `write_gate` call,
which is the read that must *not* be mistaken for the key's — with a `checked == 1` floor so a
rename leaves it failing rather than asserting over nothing.
**The fetch effect therefore lives *above* that container and keys on the target alone.**
`hidden` moves whenever a form or the preview opens *or closes*, so an effect created inside the
child re-ran on every one of those transitions: a full `fetch_principals` — a fresh connection,
through the SSH tunnel if there is one, plus an unbounded `SELECT … FROM mysql.user` — for
pressing Cancel, and two for an Apply. The comment that stood there said the effect "runs once
per opening: the closure reads no signal", which was true of the closure and not of the scope it
was created in. It compares against its own previous value as well, since `create_effect` has no
equality check and the target signal is written with the value it already held.
**And the answer is guarded by `overlay.users_generation`**, `DdlUi::generation`'s pattern for
the same failure one modal up. Two `fetch_principals` are routinely in flight on an *identical*
target — creating an account is one, closing the preview afterwards is another — and target
identity was the reporter's only guard, so the later request was not guaranteed to be the last
writer and the list could settle on the **pre-mutation** snapshot: an account just created
missing, or one just dropped listed again with a live Drop beside it.
**The footer's Refresh resets the selection and the grants, not only the list**, and it was the
one path that did not. Its stated job is noticing a `DROP USER` applied from another client, so
the account it names is precisely the one the refresh may be about to find gone — left standing,
the detail pane went on describing a dropped account with a live **Drop** button over it while
the list beside it no longer had the row. The app's DDL-apply path does exactly this and says the
same thing.
**Opened from the SCHEMA gear and from a right-click on the tree's blank space, for the reason
`Create database` sits in both**: an account belongs to the *server*, not to any row in the tree,
so the modal has no object and takes none, and the gear is not a duplicate of the blank-space
entry because a connection whose tree already fills the panel leaves no blank space to
right-click. In the gear it sits **above** `Create database` — a read entry before a write one,
skeleton group 2 before group 4. It shipped below it, which is the cross-group inversion
`menu_order_gate` exists to catch and cannot see in this menu, so the gear now reads Refresh ·
Collapse all · Users and privileges · Server variables · Create database · Show table sizes. The
entry
is **absent** where the engine has no accounts at all (`users::supports_users` — SQLite) and with
no saved connection (`ctx.exists`), and **dimmed on a down connection but not on a read-only
one**: browsing accounts writes nothing, so the read-only refusal that guards `Create database`
would be answering a question this action does not ask, while a connection that cannot be reached
would open the browser onto a fetch that fails. `open_for_server` is the one door; it takes
`(ConnUi, OverlayUi, conn_id, database)` rather than the `UsersCtx` its neighbours take, being
the only function here that touches neither fetch and so having no reason to clone two `Rc`s to
write five signals — its two call sites (`overlays.rs`'s gear entry, `schema_tree.rs`'s
blank-space menu) pass `ui.conn, ui.overlay`. It resets
every signal it reads on the way in rather than on close, so a second opening cannot flash the
previous server's accounts while the new list is in flight; the database it is given is the
active tab's, because that is the one PostgreSQL's schema and table privileges can be read from
— and, on SQL Server, the one whose users are listed at all (`Db::fetch_principals` takes it).
Two panes: a filtered account list (`users::filter_indices` over `matches`, one field over the
whole `app@host` display name, asked **once** and read by both the rows and the footer's count)
on the left, and the selected account's attributes and `GRANT` statements on the right. A
`Principals::note` — the MySQL ladder reaching its last rung, or SQL Server's partial
listing — is rendered as a faint row
**under** the list, the way the privileges pane renders `Grants::note`, because the footer's
"1 account" is exactly as confident about a denied read as about a server with one account.
**The list column runs the full height of the body and the footer belongs to the right column**,
which is Manage Connections' shape and was adopted from it: a footer spanning both put a rule
through the list just above its last row, and the count under a list the count is not about. The
column keeps its `border_right`, so the only line crossing it is the one dividing the two panes.
Inside it the search box is full width (placeholder `Search accounts`), the list sits 10px below it
with 5px between rows — Manage Connections' figure, two lists of names in one app spaced the
same — and `+ New account` is an `in_ring_button` row pinned at the foot: `CIRCLE_PLUS` plus accent
text at `menu_item_style`'s 12px inset, the shape and the place `New connection` has — and the
same face, `widgets::menu_row_face`, so the label ends in a `…` at the column's `list_w()`
(`scaled(260)`) rather than running past it.
**The list is keyed on `(state, filter)` and deliberately not on the selection** — picking a row
would otherwise throw the whole list away to change one row's background, and take the scroll
position under the pointer with it — so the selected row's background is a *reactive style* over
`overlay.users_selected` instead. That closure reads it with **`with`, never `get`**: it runs for
every row on every restyle, hover included, and `get` clones the whole `Principal` and its
attribute vector to answer one equality test — the per-item case the Floem gotcha below is
about. **The rows wear the connection list's affordances, copied
rather than approximated**: resting `theme::conn_list_text()`, hover brightening the *text* to
`conn_list_sel_text()` with no background behind it, and selected being that same bright text on
a full-width `conn_list_sel_bg()`. A row is full width and carries the 12px inset as its own
padding — the column carries none — because otherwise a selected background stops short of the
column's edges; the search box repeats the inset for the same reason. A `system` account stays
`text_faint` in **every** state, and that colour is set on the label rather than on the row, so
the row's own colour cannot overrule it. Copy is always enabled and a no-op before a selection,
the same bargain the properties modal's Copy makes: the alternative is a button whose enabled state is a
`dyn_container` over the fetch, rebuilding its focus-ring registration on every click. It copies
what the pane shows, which is already redacted, because redaction happens at the fetch.
**Each `GRANT` is rendered with `widgets::highlight_sql_mono`** — base `theme::text` on a
`bg_deepest` block (`statement_row`), the same call Query History, the snippet library and the
AI chat's fenced SQL make, with the dialect read off **`UsersTarget::dialect`** — captured at
open beside `conn_id` and for its reason, since the statements being highlighted belong to the
server the browser was opened on rather than to whichever connection the switcher has since
moved to. Its two sibling targets (`AccountTarget`, `GrantTarget`) carry one for the same
reason. It was a plain monospace `text()` on `code_bg`, and then the editor theme's palette on
the editor's surface (`theme::preview_bg`/`preview_fg`) — an editor-coloured block in a
UI-themed form. It now takes the interface theme's SQL palette on the **field surface**, as the
DDL preview's SQL box does, since it sits in a form among fields; `contrast.rs`'s `UI_PAIRINGS`
measures the five `sql_*` roles on `bg_deepest`. The trade is that a `GRANT` no longer reads
exactly as in the tab it would be pasted into: the same lexer, but five colours folded from the
editor theme's full set, and no bracket colouring.
**Each write action sits beside the thing it acts on**, not in the footer: `+ New account` at the
foot of the list column, under the list it adds to rather than beside the box that searches it,
so the column reads top to bottom as *find one, or make one*; and a `Privileges` /
`Reset password` / `Drop` row under the selected account's name. That row is an `Option<AnyView>`
the detail pane
**extends its section list with**, never an `empty()` placeholder: the stack has a 16px gap and
floem gaps an empty child like any other, so an absent actions row left a hole between the
account's name and its attributes — the trap `properties::stats_body` states. **Reset password is
absent on a role rather than dimmed**, the call every per-engine and per-kind affordance in this
crate makes: a role takes no password on either engine, so there is nothing there for a dimmed
button to promise, and the question is asked as `users::supports_password_reset` rather than as a
`dialect ==` — that one capability folds "does this engine have accounts" **and "can this
catalogue tell a role from a locked user"** into the same answer. The second half is why the
button hands it the whole `Principal`: asking the row's `PrincipalKind` put the offer on a MySQL
8 role, which publishes no `is_role` and is therefore labelled `User`, and 8.4.11 accepted the
`ALTER USER` behind it — see `core::users` for the measurement and the fingerprint that now
withholds it. **Privileges asks `users::supports_grant_to` on the same terms**, and
`account_editor::open_for_grant` refuses the door on the same answer. It was absent on a SQL
Server login while only database levels were offered — `GRANT … TO` a login at one is Msg 15151
— and is offered there now that the server level is, the answer being computed from
`levels_for_account` rather than kept as a list of exceptions. A
login row wears `icons::KEY_ROUND` where a user wears `USER` and a role `USERS`, the key its
users sign in with; a user mapped to a login of another name reads `name ← login`
(`Principal::display`), so the link is on the row itself.
The row sits in
**the modal's own `FocusRing`, at 12, 13 and 14**: they built a `FocusRing::new()` of their own
with no focus root stepping it, so clicking one focused it and Tab then cycled the pair forever
with Escape the only way out. 1 and 2 would put them *before* the search field, so they take the
stops after the list's last fixed one (11) and before the footer's `ACTION_TAB`. Reset password
took 13 and moved Drop to 14, so the Tab order follows the row and Drop stays last of the three
because it is the destructive one. The footer's
actions are about the *modal* —
re-read the server, copy what is shown, close it — and a Drop down there would sit one Tab from
Close, which is the wrong pair of neighbours for an irreversible action. **Refresh is the way
back to the truth**: everything else here re-reads the server only as a side effect of opening
something, so a list gone stale — a `DROP USER` applied from another client, a fetch whose answer
lost a race — could otherwise only be corrected by closing the browser and reopening it.
`WriteGate::of` is asked **once, in
`users_overlay`**, and the answer is passed into both panes: the two rows asked it independently
to begin with, which is two places for one answer to drift — one of them leaving `+ New account`
live while `Privileges`/`Drop` were dimmed, with nothing on screen to say which was right — and
it re-walked the connection list for an answer that cannot differ between them. Its capability
half reads `target.dialect` and its read-only half reads the **browser's own** connection's flag
live, tracked: what the browser is about cannot change while it is open, and a read-only setting
can. Both halves being the *target's* is the repair — the read-only one asked the **active**
connection, and nothing in `switch_conn` closes this overlay, so opening Users on a read-only A
and then moving the tree to a writable B left the browser listing A's accounts while **Drop** and
**Privileges** lit up and the "This connection is read-only" note disappeared. No write escaped
(`ddl_preview::apply` re-asks `plan_read_only` by the *plan's* `conn_id`, which is A's), so the
cost was an enabled destructive button and a red confirm on a connection the app would then
refuse. `target.dialect`, `target.database` and `target.conn_id` were already the captured ones by
`UsersTarget`'s own rule; `read_only` was the one term that was not, and it goes through
`connection::read_only_of` like every other reading of that flag. Its helpers take the
`RwSignal<Vec<Connection>>` rather than `&Ui`, which is `whole_ui_gate`'s own prescription and why
this file's budget went 9 → 8. It gives
**three refusals with three remedies**, because they have three different answers: no
engine support is *absent* (there is nothing about this connection the user could change),
while read-only and no-database-selected are both dimmed and each say under the buttons which
one it is. The last is the reason the sentence is there at all: an account change takes the
ordinary in-database route (`ddl::is_account_change`), so with nothing selected there is nowhere
to send it, and a dimmed pair with no explanation reads as a bug. Actions are
**never offered for an account the server maintains** (`Principal::system`, the flag that
already dims its row): dropping `mysql.sys` or `pg_monitor` breaks the server rather than the
account, and no privilege screen should make that one click away — the pane says so in a line
instead. Drop goes **straight to the preview** (`ddl_preview::preview_account`), whose warning
is the change's own `Change::risks` in the target's dialect. It used to raise a `Confirm` first
whose body was that same sentence (via `overlays::risk_prompt`), so the preview's Apply asked
the question a second time with the same words; the confirm is gone, and `risk_prompt` with it.
**It guards its own launch** — `widgets::accept_launch` at the click — and the live `read_only`
read there is what goes into `ddl_preview::PlanTarget` instead of the `false` that was written
there. The second ask that lived in the confirm's `resolve` went with the confirm: there is no
deferred step left for the flag to flip during, and the preview's Apply re-asks
`plan_read_only` for the plan's own connection when it is pressed. The rule and the bug are
under *Architecture invariants*; what is worth knowing here is that the `enabled` this row dims
itself with is a `bool` captured at build time, so it can never be the guard.
**Reversing a listed `GRANT` into a `REVOKE` by parsing it was built, tested and then removed.**
It is the feature the right-hand pane invites — a button beside each statement — and it needed
either a raw-statement `Change` variant, which would have become the escape hatch every
structured change routes around, or a parser whose wrong guess emits a `REVOKE` that takes away
more than the line the user clicked. The grant form serves both directions instead, which is
what its Action toggle is for.
**Its words are the catalogue's** (`users-*`) — the search placeholder, the
list's notes, sections, the system-account note, the action row, the write-gate notes, Copy
privileges and the statements-capped note — with the title the tree's own `tree-users`, Drop
and Refresh `tree-drop`/`tree-refresh`, and Close `dialog-close`; the Privileges button reuses
`users-section-privileges`, the heading of the section it edits. The footer's count is a Fluent
plural, and `{shown} of {n} accounts` while a filter is narrowing it. The kind under an
account's name is `principal_kind_label(PrincipalKind)`, `PrincipalKind::label`'s words drawn
here (the core-words rule under *UI conventions*).
`english_names_every_principal_kind_as_core_does` holds English equal to core's for all three
kinds and pins the count and the no-match note — the
last because the source spelled its quotes as `\u{201c}` escapes, which a literal comparison
cannot see, and the catalogue spells them typographically. The local `section` takes a `String`.
The two notes — `Principals::note` under the account list, `Grants::note` under the privileges —
are core's `PrincipalsNote`/`GrantsNote` values, worded by `principals_note_text`,
`grants_note_text` and `pg_implicit_text` from `users-note-*`. Where core's English is several
sentences — SQL Server's listing and short-list notes, PostgreSQL's implicit holdings ahead of
its scope note — each is a message of its own, joined with a space in code as core joins them.
The owned-object count and `-ms-unshown` are plurals, and with no database selected the count is
`-pg-owns-cluster`, a whole message, rather than *this cluster* handed in as `$database`.
Database, role and securable-class names go in as the server spells them, the classes joined
with `", "` in code. `english_says_every_users_note_as_core_does` holds every one equal to
`message()` — every `MsListed`/`short` pair that has a note, and PostgreSQL's scope with a
database and without under each implicit holding. An account's *Attributes* are core's
`AccountAttribute` values, worded by `attribute_text` from `users-attr-*` — `users-attr-yes`/
`-no` for a flag, `users-login-type-sql`/`-windows`/`-windows-group` for SQL Server's login
types — with the server's text inside each (a plugin, a date, a login, a schema, a role list, a
type letter it has no word for) passed through as it came, and a connection limit ungrouped as
core prints it, a limit being a setting rather than a figure.
`english_words_every_account_attribute_as_core_does` holds the pair equal to `label`/`value`
over every variant, both values of a flag that carries one and every login type. The fetch
errors stay English: the server's text.
- The two requests are tracked apart — `UsersState` for the list, `GrantsState` for one
  account's privileges — for the reason the properties modal's exact `COUNT(*)` is tracked apart
  from its statistics: a failure to read one account's grants must not replace a list of accounts
  that loaded fine. `SchemaActions::principals` asks `supports_users` before the round trip and
  reports `UsersState::Unsupported`, which is a different thing to say than a fetch that failed
  and is distinct from an empty `Loaded` (that would read as "a server with no users").
- Both callbacks outlive a close, so each checks `overlay.users` still holds the target it was
  asked about; `grants` additionally checks `users_selected` is still the same account, since a
  second click while one is in flight would otherwise land the wrong account's privileges in the
  pane. Nothing is persisted — the browser opts out of `SavedTab` by construction, as every
  other modal does.
