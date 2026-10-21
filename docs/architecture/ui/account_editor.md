# `ui/account_editor.rs`

`account_editor.rs` — the Users and privileges browser's **write half**: two overlays in one
module, `account_editor_overlay` (create an account, or reset one's password) and
`grant_editor_overlay` (grant or
revoke), both raised from `users_view` and both ending at `ddl_preview::preview_account`, which
is the third sibling of `preview_change`/`preview_container` and the only thing here that runs
anything. Mounted in the modal layer's DDL group sharing `object_editor.rs`'s tuple element,
counted by `ddl_editors_up`, and on `PAINTS_A_BACKDROP` as one file with two overlays — **two
overlays behind three doors**, since the account form has two modes and an opener for each. The
read-only refusal is inside `open_for_new`/`open_for_reset`/`open_for_grant` rather than at the
button, the same
rule `database_editor::open_for_new` follows: a launch guards itself in the step that launches
it, and the browser's dimming is what *says* the action is unavailable. **`open_for_reset`
carries a second refusal of that shape** — `users::supports_password_reset` against the
`Principal` it was handed, not against its `PrincipalKind`, since a MySQL 8 row the catalogue
cannot tell from a role must not reach a form whose preview would come back empty; `core::ddl`'s
`SetAccountPassword` arm is the third ask of the same question.
**A form's *address* comes from the browser that raised it, not from the connection switcher.**
Both launchers take the browser's own `&UsersTarget` and stamp `conn_id` and `dialect` off it;
they read `edit_ctx` instead, which resolves `ui.conn.active_conn` at the moment the button is
pressed, so a `GrantTarget` paired a `Principal` read out of connection A's `mysql.user` with B's
connection and B's grammar — the plan was emitted at B's dialect and `GRANT … TO 'app'@'%'` ran
on B for an account that lives on A. `UsersTarget`'s own doc had already stated the rule in the
imperative ("the browser describes the server it was opened on, even if the switcher has since
moved", and that its two sibling targets carry one for the same reason), and the module's *third*
write action, Drop, was already spelled this way eight lines below. **`read_only` was left live
on the argument that it is the refusal rather than the address, and that argument was wrong.**
The browser's three buttons are lit from the *target's* flag (`users_view::write_gate` →
`target_read_only` → `read_only_tracked(conns, target.conn_id)`) and nothing closes the browser
when the schema tree switches connection, so opening Users on a writable A and switching the
tree to a read-only B left **+ New account**, **Privileges** and **Reset password** lit from A
while all three returned having set nothing about B: a lit control that does nothing, with no
modal, no message and no dimming, on the three writes the browser offers. The same row gets it
right one button along — **Drop** asks `users_view::launch_read_only(conn.connections,
plan_conn_id)`, "about the connection the plan is for, which is the one the account lives on" —
so that button's answer is now the doors' answer too, through `account_editor::door_read_only`,
which is a one-line call into `launch_read_only` rather than a fourth spelling of the question.
**Both terms moved, not only the refusal**: the doors also stamped `read_only: ctx.read_only`
into the target they open, so refusing on the target while stamping the switcher's flag would
have swapped one wrong outcome for another — the connection is read once, bound once, and used
for both. `edit_ctx` is gone from all three doors, `read_only` having been the last thing they
took from it. `read_only_door_gate` therefore finds this file's doors by a second
`(stamp, refusal)` pair, `read_only: door_read_only,` / `if door_read_only {`.
`account_editor::anchor_gate` holds both
halves,
and it is a **source** gate because the decision is struct literals inside `fn`s that write a
reactive bundle. When it was written those `fn`s took the whole `Ui` — 36 fields and 91 more
transitively to build in a test; the doors take `(ConnUi, DdlUi, …)` since this file came off
`whole_ui_gate`'s list, which is smaller and still a `DdlUi`'s 40 fields inside a Floem scope. What it can see
mechanically is the spelling — `ctx.conn_id`, `ctx.dialect` and now `ctx.read_only` must not
appear in this file's
production code, `conn_id: from.conn_id,` and `dialect: from.dialect,` must appear once per door,
and the file must still ask a question in `edit_ctx`'s place, which is the floor that stops a
rename leaving nothing to look for. **That floor has moved twice, and both moves are the same
hazard.** It asserted `edit_ctx(ui)` until `edit_ctx` narrowed from `&Ui` to `ConnUi` under
`whole_ui_gate`, at which point the gate failed on a signature change that touched nothing it
guards, and it was made argument-agnostic (`edit_ctx(`) for that; then the doors stopped calling
`edit_ctx` at all, so the floor is
`door_read_only(` — the question asked in its place. A source
gate written against how a call is spelled is coupled to every refactor of the callee, not only
to the behaviour it exists to protect. The gate's message used to end "`read_only` is the one
field that stays live — it is the refusal, not the address", which read as a distinction and was
the hole itself. **The door count is a `DOORS` constant, not a literal in
each assertion**: it went 2 → 3 when `open_for_reset` landed, both halves of the gate caught the
new door, and a fourth `open_for_*` raises the number in one place. **That third door was paid for rather than
added**: another `ui: &Ui` would have raised this file's `whole_ui_gate` budget, and the
ratchet's only legal direction is down, so `suggested_field` and the
`table_designer::suggest_chevron` it
wraps (through `suggest_field` now) both narrowed from `&Ui` to `crate::OverlayUi` — the two `Copy` signals (`popup_menu`,
`popup_anchor`) are the whole of the chevron's interest in the bundle — across four call sites,
which is what takes `table_designer.rs`'s entry 33 → 32 in a change about accounts. That entry
went to **29** next: `clamp_selection` and `swap_selected` narrowed to `DdlUi` — every signal
either of them touches is in that one child bundle, which is the ratchet's own prescription —
and `default_type` went away outright rather than being narrowed, its decision now
`ddl::default_new_column_type` with a test behind it. `DdlUi` being `Copy` is what let the
columns list stop cloning the whole `Ui` four times to reach it. It reached **10** when the
campaign written up under `lib.rs` took the file's whole form-and-list half to `DdlUi` and its
field helpers to `RwSignal<TableDraft>`, **4** when the six shared readers came down after it,
and **zero** when the three opening paths and the overlay followed and the file left the list
altogether; the `lib.rs` write-up carries the steps, and this sentence said 10 for a while after
that stopped being true.
Scoped to this file on purpose: every other editor is launched from the
schema tree, where the active connection *is* the target.
The account form **creates, or resets one account's password — and never renames** — near enough
the shape `database_editor` has and for the same reasons: an account is dropped from its own row
in the browser, and neither engine offers a
rename that is safe to perform. **Which of the two it means rides on `AccountTarget::resetting`**
(`Option<Principal>`), not on whether some field happens to be filled: `account_change(draft,
resetting, salt, policy)` reads the statement's *subject* from there, so a reset cannot rename or re-host the
account it is resetting even though the form seeds the draft's name and host in order to say
whose password it is — reading them back would do it silently, `ALTER USER 'b'@'%'` on an account
that does not exist being an error the preview would blame on the server. That mapping is
invisible in a rendered form, which is the kind that ships backwards, so
`account_change_tests::a_reset_names_the_target_rather_than_the_draft` and
`the_reset_the_form_builds_emits_the_alter_for_that_account` pin it — the second over the
composition rather than the function alone. A reset is **a second mode, not a second modal**: the
body is the account's name as static text plus the one password row, the title reads "Reset
password" (a form that will emit an `ALTER` under "Create account" being the one thing the title
may not do), readiness and the status line ask a different question per mode — a create needs a
name, a reset needs a non-empty password, because `set_password_sql` refuses a blank one and the
plan would otherwise come back empty and read as the app being broken — both through
`users::account_form_ready`/`account_form_blocker`, so the button and the sentence under it
cannot disagree, and `password_row` was
extracted so both modes wear the one `masked_edit_field`, which is why that widget is
`pub(crate)`.
In create mode its Kind picker comes first because it decides what the rest of
the form means: a role takes no host and no password on either engine, so those fields **vanish
rather than sitting there inert**, and Host is absent on PostgreSQL, which has no such thing at
all. **On SQL Server the Kind list is `users::account_kinds` — Login, User, Role — and the form
opens on a Login** (`users::blank_account_draft`), with an *Also a user in `<db>`* toggle,
on by default, that adds the same-named user to the plan (`preview_changes`, which Preview
hands to `ddl_preview::preview_account`; never beside a reset). On Azure SQL Database the list
is User and Role and the form opens on a User, its logins not being managed here (under
`core::users`). A User gets a *For login* field instead, empty meaning
`WITHOUT LOGIN`. The password row follows `users::draft_takes_password` rather than the Kind being
`User`, since on SQL Server it is the login that holds the password and a user has none of its
own — **except in a contained database**, where the row appears for a User too, and a For login
left empty with a password filled makes it a contained user (`CREATE USER … WITH PASSWORD`).
A password the rows no longer show — typed for the Login the form opened on — is left out of
the Preview and its blocker (`users::account_form_as_shown`, through the private `shown`).
**Where the connection signed in through Entra (`users::supports_entra_users`) a User also gets
a *Microsoft Entra* toggle**, and turned on it takes *For login* and the password rows away with
it — an Entra user (`CREATE USER … FROM EXTERNAL PROVIDER`) has neither — which is why
`account_form_shape` is `(kind, external)` rather than the Kind alone (below).
Whether the database is contained, whether its logins live elsewhere and whether Entra users
may be made ride on `AccountTarget::scope` (`users::AccountScope`), which
`users_view::loaded_scope` reads out of the loaded list beside `loaded_policy` (below) — the
all-`false` default until it has loaded, so the contained user's password row and the Entra
toggle stay off rather than appearing for a database the server would refuse them in; a reset
opens with the default, since it is of an account that already holds its password and whether a
new one could is not its question. Both SQL Server rows are asked of whether the engine's kinds
include `Login` — accounts split in two — rather than of the engine, and **deliberately of the
engine's kinds (`account_kinds(dialect, Default::default())`), not this database's**: where the
logins live elsewhere the Kind list has no Login, yet a user there is still made `FOR` one, so
asking the scoped list would take *For login* off Azure with it. **The form holds a password — in `account_draft` and, typed a second time, in
`DdlUi::account_confirm` — and nothing else in this crate does.** Both are blanked on every
open — a form that reopened holding the last one would put a credential on screen nobody typed
this time — cleared on Cancel (`ddl_preview::close_peers` empties `account_draft` and
`account_confirm` with the target, since both are app-lifetime and the target is not), never
persisted and never logged; the confirmation never reaches a plan, `account_form_ready` being
its only reader. The password becomes visible in
exactly one place: the preview's SQL. That is deliberate. The preview is the app's one gate
between a plan and a server, and a statement shown there with a field blanked out would not be
the statement it ran — but nothing *leaves* the preview carrying it, which is
`ChangeSet::export_script`'s job. **On PostgreSQL the preview does not show it either**, for any
password SASLprep leaves alone on a server with no password-check extension loaded: Preview SQL
calls `preview_change(draft, target)`, which is `account_change` with `fresh_salt()` — 16 bytes
from `getrandom::fill` — and `target.password_policy`, and the salt stamped on either change
makes the statement carry a verifier instead — `core::scram`'s at the server's own
`scram_iterations`, or an `md5` one on a cluster kept on md5 — so what the preview shows is what
the server logs and neither is the password. The policy comes from the browser:
`users_view::loaded_policy` reads it out of `UsersState::Loaded` when **+ New account** or
**Reset password** is pressed, `None` before the list has loaded, and `open_for_new` /
`open_for_reset` put it on `AccountTarget::password_policy`, captured with the target's other
fields. The salt is handed in rather than drawn inside `account_change` so the mapping
stays pure, and it is fresh per press because two previews of one password sharing a salt is
the salt no longer doing its job (`every_plan_gets_its_own_salt`). An OS with no randomness gives
`None`, which is the password as typed and never a lockout. `password_row` takes the dialect and
renders `users::password_hint` beneath both fields, so the line says which of the two the
preview will hold. `the_form_hashes_a_postgres_password_under_the_salt_it_was_given` is the composition
test: the emitter's own tests prove a salted change becomes a verifier and this proves the form
builds a salted one, for create and reset alike — a stamp lost between the two puts the password
back in PostgreSQL's logs with every other test green, and an `account_change` that dropped the
salt was seen failing it. **That test still left the salt's own call site unreached**: the
button called `account_change(.., fresh_salt(), ..)` inline, and `None` substituted there kept
the suite green while every PostgreSQL password went back into the server's logs as text —
`None` being, by design, the silent fallback. `preview_change` is that call site as a function,
and `the_preview_change_carries_a_verifier_not_the_password` pins it for a create and, under an
`Md5` policy, a reset; it was watched failing with `None` in place of `fresh_salt()`.
**The password is typed twice, in a masked Confirm field at tab stop 31 under Password's 30,
because nothing else shows it.** The field is masked and on PostgreSQL the preview now carries a
verifier rather than the text, so a slip — `hunter3` for `hunter2` — was visible nowhere before
an irreversible reset, after which whatever was configured with the right password could no
longer connect. The footer's reason and Preview's enable both come from
`users::account_form_blocker`/`account_form_ready`, so an empty Confirm reads "Type the password
again to confirm it." and a mismatch "The two passwords differ." — checked by hand in the
sandboxed app, where a match enabled Preview. A role has no password row and is not asked, nor
is a SQL Server user outside a contained database, nor an Entra user; a SQL Server login must have a password as
well as its confirmation ("A login needs a password."), and a user for a login must not have
one ("A user for a login signs in with the login's password — leave this blank."). An Entra
draft that is not a User, or that still holds a login or a password typed before its toggle hid
them, reads "Only a database user is made from Microsoft Entra." or "An Entra user has no login
or password here — Entra signs it in."
**The field itself is
`connection_form::masked_edit_field`**, the same one the three saved-connection secrets wear:
this was the app's only *unmasked* secret field, so its real characters were on screen and a
Ctrl+A/Ctrl+C from the clipboard. That helper is `pub(crate)` precisely so there is one of them
— a second masking widget is how the two come to disagree about the replay rule, which is the
part that is easy to get subtly wrong, and it is the same widget that reaches
`CREATE USER … IDENTIFIED BY` and `ALTER USER … IDENTIFIED BY`. **This form is why the replay has
to be exact rather than close**: the connection form's mangled password fails to connect and can
be retyped, while a mangled reset locks the account out of whatever was using it.
**Every fixed-list choice in both forms is the app's `<select>`** — `settings::focusable_dropdown`,
the control the settings modals wear, so the popup, the keyboard, the tinted current value and
the chevron box are one implementation. Four rows moved onto it: the account form's **Kind**, and
the grant form's **Action**, **Subject** and **Level**, all of which were rows of `action_button`s
that read as picked or not, and the module's own `toggle_button` is gone with them.
`bound_dropdown` and `bound_toggle` sit beside `bound_field` and exist for the reason it does:
`focusable_dropdown`/`focusable_toggle` bind to an `RwSignal<T>` and these values live in a
**field of a draft struct**, not in a signal of their own, so each seeds a local signal once and
writes back through an effect only on a genuine change — a rebuild cannot read as an edit.
`bound_dropdown` takes `label` as a `fn` rather than a closure because `focusable_dropdown`'s is,
and wraps the control at `field_w()` so a form of mixed controls lines up.
`picked_outline(style, picked)` survives the move with **one** caller left, `privilege_tag`:
`theme::accent()` when chosen and `Color::TRANSPARENT` when not, so only the colour changes.
Taffy sizes the border box, so a 1px rule added *only* while picked grows a button sized by its
own padding by 2px, which is what the Kind, Level, privilege and option toggles all did the
moment they were clicked while they were buttons — and in a wrapping cloud it re-flows every tag
after it on the line. It is the same accounting `widgets::row_menu_mark_pad` does one level down,
taken from the other end: a tree row's height is fixed by what surrounds it so the 2px is given
back out of its padding, while a button here has nothing to give back, so the border has to be
there all along. See the UI convention.
The grant form is **one form for four statements** — grant or revoke, privileges or a role —
and the mapping from its Action and Subject dropdowns to `Change::{Grant,Revoke}{Privileges,Role}`
is `core::ddl::grant_change`, pure and unit-tested rather than four arms in the render.
**The Action dropdown is over a `bool`**: `GrantDraft::revoke` stays the `bool` that
`PrivilegeChange` and `ddl::grant_change` read and that their tests pin, and the free
`action_label(bool) -> String` gives it its two words. An enum invented for the form would
be a second spelling of the same fact sitting one conversion away from the tested one.
It opens pre-picked to the widest level **below the whole server** (`users::default_grant_level`,
asked of the account — a SQL Server login, whose one level is the server, opens there:
`a_sql_server_login_opens_on_the_server`), and its Level picker lists
`users::levels_for_account`, so a login is offered only the server and a user never is. So the name fields mean something before the user has noticed the picker — it read
`levels_for(dialect).first()` until that argument turned out to be true on PostgreSQL and false
on MySQL, whose list starts at `Global`, a level with no name fields to mean anything: the form
opened already satisfied at server scope and two clicks emitted `GRANT … ON *.*`. And **changing
the level clears the ticked privileges**: kept,
they would carry `EVENT` down to a table level that has no such privilege and emit a statement
the server refuses. That pre-picking is `initial_grant_draft(dialect, account)`, its own function beside
the openers rather than a literal inside `open_for_grant`, because the **Level row exists only
when the draft holds a level** — `if let Some(current) = seed.level`, and no row at all when it
is `None`, since a dropdown with nothing in it is a worse answer than no row. **The row has no
fallback of its own, deliberately**, and that absence is the first thing a future editor would
tidy back in: an `or_else(|| levels.first().copied())` here would paint a level the draft does
not hold, while the fields below stay gated on `seed.level` and so stay hidden — and because
`bound_dropdown` writes back only on a *genuine change*, picking the very entry already shown
would not be the change that unstuck it. With the seeding in one named function, `None` means
exactly *an engine with no levels*, which cannot reach this form at all
(`users::supports_user_admin` gates the browser's button). The coupling is pinned by
`the_grant_form_opens_holding_a_level_wherever_the_engine_has_one` and
`the_level_it_opens_on_is_never_the_whole_server`, which pins the *composition*: the capability
answering `Database` is worth nothing while the opener still reads the picker's first entry.
**The privileges are a wrapping tag cloud** — `h_stack_from_iter` under `FlexWrap::Wrap` at 6px,
where they were one per line. Eighteen is a legal selection at MySQL's database level, and
eighteen rows is a column of short words taller than the panel: a set you have to scroll to see
the shape of. Wrapped, the whole set is one block, which is the question the row is actually
asking — *which of these*. `privilege_tag` (once `privilege_row`) still reads the draft inside
its own style, so clicking one tag does not rebuild the cloud of eighteen, and it is the one
control here still wearing `picked_outline`. **The cloud gets a reserved Tab span rather than
the distance to whatever was written next** — `PRIVILEGE_TAB` (50) plus `PRIVILEGE_TAB_SPAN`
(200), the toggle after it claiming the end of that span, with
`PRIVILEGE_TAB + PRIVILEGE_TAB_SPAN < widgets::FIXED_TAB_END` asserted at compile time. Today's
longest list is eighteen, so nothing collides — but `users::privileges_for`'s own doc calls that
list "curated, not exhaustive" and names the dozens left out, and adding 23 of them would put a
tag at exactly the toggle's index, where `FocusRing::register` inserts *after* an equal index
and `focus_at` resolves to whichever was built first. The block cannot move above the toggle
(the toggle comes after it on screen), so the span is what makes the collision impossible;
`every_privilege_list_fits_its_tab_span` asserts every dialect × level fits it. Both overlays
wear one `modal_shell` — title bar, body, footer, backdrop and the Escape handler — taking a
`ShellParts`/`ShellSize` pair rather than eight positionals, because three same-typed `AnyView`s
in a row transpose silently.
The database the browser is scoped to is *suggested* beside the qualifier
field rather than filled in, since a prefilled name on a form that grants privileges is a value
nobody read.
**Both forms' `dyn_container`s are keyed on a memo over the form's *shape*, never on the draft
signal** — `account_form_shape` (`(kind, external)`, the Entra toggle deciding rows as the Kind
does) and `grant_form_shape` (`(subject, revoke, level)`),
two pure functions naming the fields that decide which rows exist, because the values in those
rows do not. Keyed off the draft, both rebuilt the entire form on every keystroke in a name
field and on every privilege tag, tearing the field down mid-word and taking the caret with it:
floem's `create_updater` does **no equality check**, so a key that merely recomputes to the same
value still fires. That is `widgets::overlay_open_key`'s bug, reproduced in two more places.
`form_shape_tests` pins the two functions — typing or tagging never changes a shape, each
dropdown does — and **that is the whole of what it can see**: whether the key closure reads
`shape.get()` or the draft is a line in a view, which is how `overlay_open_key`'s own pin, taken
on the memo in isolation, let the regression walk past it. Preview SQL is gated on
`users::GrantDraft::is_ready`, the same predicate the footer's sentence is written from, and the
press then asks `ddl::grant_change`, which answers `None` for exactly the drafts that predicate
rejects — the doubled refusal every launch here makes, since a disabled button is not a guard.
**"With grant option" and "With admin option" are the app's switch** — `settings::focusable_toggle`
through `bound_toggle`, reading the draft, so a yes/no in this form reads as a yes/no everywhere
else one appears. They shipped as an always-"Yes" button whose `current` parameter was ignored,
so the two rows that exist only to show a state showed none of it: nothing on screen said whether
the statement about to be previewed would carry the clause. "With admin option" is absent where
`users::supports_role_admin_option` says the engine's membership has none — SQL Server's
`ALTER ROLE … ADD MEMBER` — and `core::ddl` refuses a role grant carrying one there regardless.
`open_for_grant` asks `users::supports_grant_to` beside its read-only refusal — every account
passes it today, a SQL Server login included now that the server level exists. The Role field's
shortcut is `users::role_suggestions`: a login's server roles are not in the browser's list, so a
SQL Server login is offered the fixed server roles instead.
**Its words are the catalogue's** (`account-editor-*`) — both forms' labels,
sections, toggles, placeholders and the chevrons' empty notes — with Cancel and Preview SQL the
shared `dialog-cancel`/`editor-preview-sql`. The titles are whole messages,
`account-editor-title-{create,reset}` and `account-editor-title-privileges` with `$account`, and
SQL Server's *Also a user in {database}* is `account-editor-also-user`
(`the_composed_lines_read_as_they_did_in_english`). **The dropdowns' words are drawn here** (the
core-words rule under *UI conventions*): the Kind list wears `users_view::principal_kind_label`,
made `pub(crate)` for it, and the Subject and Level lists `grant_subject_label` and
`grant_level_label` in place of `GrantSubject::label`/`GrantLevelKind::label`, with
`english_names_every_grant_subject_and_level_as_core_does` holding English equal to core's.
`grant_level_label` also names the qualifier field (*Database*/*Schema*) and the Table or
Sequence name field, so a field reads in the Level dropdown's own words rather than a second
spelling of them. `action_label` returns a `String` for the same reason, `suggested_field`'s
`empty_note` is a `String`, and the Host and *For login* placeholders go through
`FieldCfg::placeholder_of`. What stays English: *Microsoft Entra*, a product's name; the
identifier placeholders (`account_name`, `role_name`, `database_name`, `schema_name`, `name`);
and the privilege tags, which are SQL keywords. Core's two decisions are worded here:
`account_form_blocker` answers an `AccountBlocker` and `password_hint_kind` a `PasswordHint`,
which `account_blocker_text` and `password_hint_text` say from `account-editor-*`
(`english_says_every_blocker_and_password_line_as_core_does` holds them to core's `message`).
