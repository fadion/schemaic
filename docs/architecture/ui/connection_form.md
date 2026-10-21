# `ui/connection_form.rs`

`connection_form.rs` — Manage Connections modal + password-mask (+ tests). The form is built
**once per open** while the list on its left keeps loading a different connection into the same
`DraftSignals` — so every control the form owns a *separate* signal for has to be synced back
down from the draft, not merely seeded from it. The password fields' `mirror_real` does that;
the **Type** picker's `DbKind` did not, and reported the engine of whichever connection was
active when the modal opened for every row the user then clicked — including which half of the
form (`server_fields` vs `sqlite_fields`) got built, so a MySQL connection could be shown a
*Database file* field and no host. Both directions now have an effect, and the write-back one
tells a **pick** from a **load** by asking `connection::same_engine` whether the stored label
still names the previous engine: on a pick it does, on a load it already names the new one.
That is also what stopped opening the form from rewriting a `MariaDB` label to `MySQL` — the
write used to be unconditional, and the picker has one name per engine where `db_type` has
several.
**The form is four sections, not one column** (GitHub #16): **General** (Name and colour,
Folder), **Safeguards** (Prominent colour, Read-only, CLI access, AI data access, Statement
timeout), **Database**
(Environment, Type, then `engine_block` — Host/Port, Database, Authentication and User/Password
for a server, the Database file for SQLite — then Init SQL) and **Network** (the SSH tunnel and its fields, SSL /
TLS). Each opens with `widgets::form_section_header` and they stand `widgets::form_section_gap()`
apart with no divider, as Settings' did while it was one column; inside a section the fields
keep the form's 20. It had
been a single 20px-gap column. For Network to be a section of its own, the tunnel and
`tls_fields` came out of `server_fields` into **`network_fields`**, which `conn_form` builds in
a `network_block` `dyn_container` keyed on `DbKind::is_networked()` — the heading inside it, so
an engine with no server loses the heading with the fields. On SQLite the child is `nothing()`
**and the container is `widgets::collapse_unless`'d on the same predicate**, which is the
half a tidy-up would drop: an empty `dyn_container` is still a box, and this one would leave a
36px section gap under Database with nothing after it. The split renumbered nothing —
`server_fields` keeps tabindex 60–80 and `network_fields` the 90–160 the tunnel and TLS
always had.
**The *CLI access* toggle is this form's whole write surface for `Connection::cli_access`**, and
it is the only one anywhere: nothing else in the app sets the flag. It sits at tabindex 33,
between Read-only (30) and AI data access (35), which is where it sits on screen — a Tab order
that disagrees with the eye makes the keyboard skip a control and come back to it, the reason
already written beside the AI picker. Its hint is *"Let the schemaic command line, and AI agents
through schemaic mcp, use this connection."*, and the three guard-rails read as one group on purpose, being the same kind of
per-connection answer. `DraftSignals::cli_access` is threaded through create, load, reset and
`to_connection` exactly as `read_only` is, and that is the part worth pinning: a hardcoded
`false` at `to_connection` would have compiled and silently cleared the flag every time a user
edited an exposed connection for any other reason.
**The *Statement timeout* dropdown is `Connection::statement_timeout_secs`'s write surface**,
last in Safeguards after AI data access, at tabindex 37 — after that picker's 35, before
Environment's 40, for the CLI toggle's reason. Its choices are `CONN_STATEMENT_TIMEOUTS`:
*Same as the app* (`None`) first, because it is what every connection saved before the field
does, then the app's own list from *Settings → Query* — none, 1, 5, 15 and 30 minutes, an hour —
labelled by `conn_statement_timeout_label` — `connections-statement-timeout-same` for `None`,
otherwise `i18n::statement_timeout`, the one renderer the app's dropdown and the timed-out
message use too — so the words match both in every language.
`connection::statement_timeout_choice_label` is the English it is held to, *Same as the app* included
(`english_names_every_timeout_choice_as_core_does`). **The list is built from `settings::STATEMENT_TIMEOUTS`** — a
`const` block prepends `None` and wraps each value — rather than copied, so a value added to
Settings appears in both dropdowns. Its hint is *"Cancels a statement on this
connection that runs longer."*, and `DraftSignals::statement_timeout` (`Option<u64>`) is
threaded through create, load and `to_connection` as `cli_access` is — the same hardcoded-`None`
hazard at `to_connection` would silently put every edited connection back on the app's value.
**The *Folder* field is free text, right after Name and Colour**, because a folder is only its
name (`Connection::folder`) and there is nothing to create first. It is still **suggested**:
`table_designer::suggest_field` over `connection::folder_names`, a ghost to Tab into and a
chevron listing the folders in use, as a column's type and a database's collation are — because
retyping a folder by hand could quietly start a second one under a misspelling (issue #8). It is
`clearable`, as the schema tree's search is: the trailing × is the visible way back to *no
folder*, which an empty box otherwise only implies — and the ghost already makes room for it,
`edit_field` passing `has_side` to the ghost's `inset_right`. The
list is read untracked from the saved `connections`, which `conn_form` takes for this alone,
with the `OverlayUi` the menu opens on, both passed from `manage_modal`. It sits at tabindex 17
— after the swatches' 15, before the first toggle's 20, the chevron taking 18 — which is where
it sits on screen, for the CLI toggle's reason; its hint is *"Optional. Connections with the same folder are listed
together."*, and `DraftSignals::folder` is threaded through create, load, reset and
`to_connection` like the rest. The list on the left iterates `connection::listed_rows`, keyed by
the `ListedRow`, and its Up/Down `nav_group` steps through `listed_ids` — the order drawn, not the
order saved, which would jump between folders. `folder_heading` (the `FOLDER` icon and the name,
in `text_faint` at `font_label`) is the one heading both lists draw: this one and the header's
`conn_menu_overlay`, which iterates `listed_rows` the same way and looks each connection up by
id. It is deliberately not a pointer or a keyboard stop — it names a group, it does nothing.
**The *Init SQL* field is `Connection::init_sql`'s write surface**, last in Database after the
engine block, at tabindex 85 — after the engine's own fields (to 80), before the tunnel's 90.
It is a multi-line mono `edit_field` coloured for the engine picked, `min_rows` 2, with the
placeholder `SET search_path TO app, public`, and it sits in a `dyn_container` keyed on the
dialect read off `draft.db_type`, because an `edit_field`'s colouring is fixed when it is built
— so changing the engine rebuilds it. **Under it is what `init_sql::check` makes of the text as
typed**: the refusal in `theme::error`, worded by `init_refusal_text`, or else a hint — both
`widgets::form_hint_style`, the refusal recoloured — *"Optional. SET statements run on every connection Schemaic opens — a search path, a time zone, a role."*, and
on SQLite the `PRAGMA` sentence. The connect refuses the same text, and this line is the only
place the user hears that before a connect fails; Save does not check it. `DraftSignals::init_sql`
is threaded through create, load (and so `blank`) and `to_connection`, and the Test result's
reset effect tracks it, since the init SQL runs inside every connect and a Test from before an
edit to it says nothing about the connect after.
**New creates the connection; it does not merely clear the form.** `new_conn` blanks the shared
`DraftSignals`, names it a unique *New connection N* with a distinct colour, mints its id through
`Connection::next_id_after`, pushes it, persists (`Saving::Replacing`) and selects it by setting
`draft.id` — the create, persist, select shape `duplicate_conn` already had. It used to stop at
the reset and leave `draft.id` at `None`, listed nowhere until Save, and a New that changed only
what was in the inputs read as a button that did nothing (issue #6). It deliberately does **not**
load the schema: the blank form points at 127.0.0.1 with no user, and connecting there is an
error nobody asked for. Until someone fills it in, `Connection::is_untouched_new` recognises it
from itself, which is how a first-run import knows the active connection is a blank standing in
for nothing, and how startup and the health poll know not to connect it (see *A first launch
saves nothing and selects nothing* below). Then it raises `DraftSignals::focus_name`, a
request rather than form state, which `conn_form` answers in an effect created *after* the Name
field — so the pulse it sends lands on Name's own effect once that has taken its first reading.
The answer clears the flag, scrolls the form to the top through a `scroll_to` signal cleared on
the next tick (the sticky-`scroll_to` trap), and pulses Name's `FieldCfg::caret_end`, so the
keyboard lands there with the caret after the default name; Name is `field` spelled out inline
for that one addition.
**A flag, not a pulse**, because the header's first-run button calls `new_conn` and only then
opens the modal: a flag left standing is answered by the form that build creates, as well as by
a form already up when New is clicked.
**The mask is replayed from the editor's own delta, never diffed out of the buffer.** The
document holds `MASK_CH` throughout, so comparing the text before and after an edit cannot tell
a mask character the *user* typed from one that was already there — and the ambiguity was not
academic: replacing `hunter2` with a pasted `***` stored `hun`, three characters of the secret
being replaced, into the OS keyring and, through the account editor which shares this widget,
into a real `CREATE USER … IDENTIFIED BY`. Nothing showed it — the mask re-rendered at the right
length and the DDL preview redacts the literal. So the field asks the editor instead:
`runs_of_delta` translates floem's `RopeDelta` into `MaskRun::Keep(a, b)` / `MaskRun::Insert(t)`
over the buffer that was there before, and `splice_real` replays exactly those runs onto the
real value. Both are pure and unit-tested here; `diff_edit` and `reconstruct_real` are gone.
Two invariants hold it up. `disp` and `real` **always share a char count**, which is what makes
a `Keep` run's offsets index both — and when they have drifted, or a run reaches past the end,
`splice_real` returns `None` and the caller restores the mask from the real value rather than
guessing, because the wrong guess here is a password nobody holds written to the keyring or to a
server. And a `MaskEdit` carries a monotonic `seq`, because two edits describing the same runs
are still two edits and pasting `***` over a three-character mask must not read as *nothing
happened*; its `edit` is `None` when one update carried several deltas at once, whose later
offsets are relative to a buffer this side never saw.
A right-click in the list on the left **selects on the action taken, not on the click**: its
menu's Duplicate and Delete carry the connection id themselves, and selecting up front meant
merely *opening* the menu ran `draft.load` over every field the user had typed, with no undo and
no copy in the keyring. What answers "which connection is this menu
about" instead is the shared open-menu rule (`widgets::row_menu_mark`, *Popup menus*), keyed off
a `menu_row: RwSignal<Option<u64>>` created in the modal's **stable scope** beside `save_flash`
and `test_flash` and for the same reason — the clearing effect outlives the open/close
`dyn_container`, and a signal built inside it would be disposed out from under that effect.
Those two flashes are the transient confirmations of the safe actions (a check standing in for
**Save**'s label, **Test**'s *Connected* on the status line beside it), and the stable scope was
only half of what they needed. `save_gen` — the generation the deferred clear checks itself
against — stayed inside the form the `dyn_container` disposes, so closing the modal inside
`SAVE_FLASH` left the pending `exec_after` reading `None` from a dead generation: it declined to
clear, and the check was still on the button the next time the modal opened. What answers it is
an effect on `open` that **withdraws the confirmation when the modal closes**, rather than
hoisting `save_gen` out beside
the flash — a check reporting a save from a previous visit is wrong even while its two seconds
are still running (the guard-scope gotcha below states the general rule). **Test has the same
shape and is deliberately not fixed**: `test_gen` is already in the stable scope so its timer
does fire, but a close-and-reopen inside `TEST_FLASH` still shows the previous visit's result
on the status line, and `test_flash` is also driven by `conn_test` state that would likely need
resetting with it.
**Test's button always reads *Test*; how the test went is a status line immediately left of
it.** The footer is Delete · spacer · status · [Test, Save], so the flex spacer pushes the line
right-aligned against the button that ran it. The button is a plain `action_button` — it used to
be an `action_face` whose face was swapped for animated dots while running and a result icon
after, and the user asked for words instead. `TestState::line(shown)` in `lib.rs` decides them,
as a `TestLine`: `Testing` (*Testing…*, `text_dim`), `Connected` (`conn_test_ok`) or
`Failed(reason)` (`conn_test_fail`), with `shown` being `test_flash`. A test in flight is not a
result, so *Testing…* ignores `shown` and stays until the answer replaces it, and a failure
ignores it too, for the reason below; only `Ok` waits on it, so *Connected* goes after
`TEST_FLASH` (5000 ms) (`test_line_tests`). `contrast.rs` holds all three to `Body` on
`bg_panel` — the failure is a sentence to be read, not a glyph to be spotted — and that is why
the running state is `text_dim`: `text_muted` was tried first and failed there, at 2.55:1 Dark
and 2.89:1 Light. The line shrinks and the buttons do not, the `modal_footer_split` contract
spelled by hand because this footer is not that widget: `min_width(0)` on every box from the row
down to the text, `flex_shrink(1)` on the status, `flex_shrink(0)` on the buttons, so a long
error ends in `…` rather than pushing Test and Save out of the panel. Its `margin_right` lives
inside the `Some` arm, since a margin on the container would leave a gap before Test with
nothing in it.
**Why the test failed is that line, folded onto one row, with the whole reason as its
tooltip.** `TestState::Fail` carries a `String` (so the enum is no longer `Copy`),
`TestState::{landed, failure}` are the two questions asked of it, and `main.rs`'s
`test_outcome` is the one mapping from the round trip's `Result`. **A result lands only on the
test that asked for it** (`TestState::landing`): while the state still reads `Testing` — any
edit, a load from the list and a reopen all reset it to `Idle` — and only for the latest press,
by a per-press sequence number in `test_conn`. It landed unconditionally, so a slow test of A
finishing after the list loaded B put A's "Connected" beside B's settings, and a failure, which
no longer times out, stayed there (`a_late_test_result_does_not_land_on_another_test`). It used
to be a bare `bool`:
`open_tunnel`'s failure arm was `Err(_) => { send(false); return; }`, which is where
`ssh::refusal_message` — several sentences naming the host, both fingerprints, that the key *"has
CHANGED since Schemaic first trusted it"*, and the out-of-band check to perform — stopped
existing. `ssh::authenticate`'s own doc names this button as the surface for exactly those
errors, and the *real* connect path kept the reason
(`Err(e) => send(Err(LoadFailure::Tunnel(e.to_string())))`) — if only as far as the log, until
`connection::TunnelFailures` — so only the diagnostic control lost the diagnosis outright: an
unreadable trust store, a wrong SSH password
and an unreachable host were one identical red X with no text anywhere. `line` collapses the
reason's whitespace, because the status is one row ending in `…` and a refusal runs to several
lines; the whole of `TestState::failure()` is the line's tooltip (the anchored `.tooltip`,
wrapped at `scaled(420)`), and that is where the host-key refusal is read now. An empty reason
reads *Connection failed* rather than opening blank
(`a_failed_test_without_a_reason_still_says_it_failed`), and is never the expected state
(`a_failed_test_always_carries_its_reason`, `an_unfinished_test_reports_neither_way`).
**The failure is deliberately *not* on `TEST_FLASH`; *Connected* is.** A failure stays until the
next test or the next edit moves `conn_test` off `Fail` (`conn_form`'s reset effect), because it
is worth going back and reading — the host-key refusal most of all — and its full text lives in
the tooltip, a child of the line. A timed failure was tried and rejected for exactly that: the
line going on schedule would take the tooltip with it mid-read
(`a_failure_outlasts_the_display_window`). **The `test_status` key reads `test_flash` only
under `TestState::Ok`**, because `dyn_container` rebuilds its child on every change of its key,
equal or not: keyed on the flash, a failure's unchanged line would be rebuilt when the timer ran out,
closing the very tooltip it stays up for.
**The SSH box's *Jump hosts* field (`jump_hosts_field`) checks itself with the tunnel's own
parser.** It sits after SSH user at tabindex 125, in the gap before the Authentication
dropdown's 130, a mono field with the placeholder *user@bastion:22, inner*. Under it is
`SshTunnel::route`'s error as typed (`jump_host_error_text`), in `theme::error()`, or else the hint *"Optional. Hosts to
pass through before the SSH host, nearest first — each signs in the way set below."* — the same
function `open_tunnel` connects by, so the form cannot pass a route the connect then refuses,
or the reverse. `DraftSignals::ssh_jump` is threaded through create, load and `to_connection`,
and the effect that resets a prior Test result tracks it, as it does every other SSH field.
**Under the SSH host/port row, `ssh_config_hint` says what `~/.ssh/config` makes of the tunnel**
— `core::ssh_config::describe_route`'s `RouteNote`, worded by `route_note_text`, so a host typed
as an alias shows the `HostName`, port, user, key and jump hosts the connect will actually use
— or, where the connect would refuse the route, that refusal, the first jump host's own route
included. The row and the hint share an inner
`v_stack` at 6px, as the Database field's hint sits under its field, rather than standing in the
SSH box's 20px gap. **The file is read once, when the tunnel's fields are built** — the `on`
arm of `network_fields`' `dyn_container`, so reopening the form or toggling the tunnel off and on
— and `describe_route` is memoised over the draft's trimmed host, port, user, auth, key path and jump,
so typing re-resolves against that text without touching the disk. The trade is that an edit to
the file shows here only on that rebuild, while the tunnel itself re-reads per connect. The
label is `widgets::collapse_unless`'d on the line being `Some`, not merely emptied: a host the
config does not change claims no 6px gap under the row.
**`tls_fields` is always visible on a networked engine, not behind a toggle like the SSH
block**: a database that enforces TLS is the ordinary case rather than the advanced one, and a
checkbox marked "use SSL" is the control that leaves people believing a connection is verified
when it is only encrypted — so it is a named level with its description
(`ssl_mode_description`, `SslMode::description`'s words) printed underneath. What it shows below the picker comes from the mode's **capabilities**, never the
variant: the CA path appears when `verifies_certificate`, the client identity when
`negotiates_tls`, so a sixth level would not need this view found and edited. An *empty* CA
path under a verifying mode means **whatever the operating system trusts** (`db/tls.rs`), and
the hint says so, because a blank required-looking field otherwise reads as unfinished.
That hint shares a 6px `v_stack` with the CA field, and the client-identity hint — about both
client rows — shares one with the Client key field in the same way: as a row of its own it took
the group's 20px gap and read as a stray paragraph.
**Under Entra sign-in with anything short of Verify full picked, a hint says the picker is not
what connects** (`tls_floor_note` → `AuthMode::tls_note` of the mode in force, hidden when it
is `None`): *"Microsoft Entra sign-in connects at Verify full
whatever is picked here: its token opens every Azure SQL database the account can reach."* The
two cloud IAM modes have notes of their own below their floors (`Require`, `VerifyCa`): AWS's
adds that Verify full needs the RDS CA bundle as Root CA, Google's asks for the instance's
server CA there. A note rather than writing the picker up to `VerifyFull`, deliberately: an effect cannot tell a
pick from a load, and raising on a load would edit a saved connection by opening it. The raise
itself is `Connection::tls_mode_in_force`, under `connection.rs`.
Every TLS signal joins host/port/user/password
in the effect that **invalidates a prior Test result**, since raising the mode or naming the
wrong CA turns a working endpoint into a refused one and a green Test left standing over that
is the most misleading state the indicator has. `path_field` is the shared path+Browse control
behind the SSH key and all three certificate paths; folding them together also fixed the
original, whose picker sat two Tab stops from the field it fills in while its own comment
claimed otherwise. Its *Browse…* sits **inside** the field (`FieldCfg::button`, under
`lib.rs`), where beside it it had stood shorter than the field (GitHub #19). SQLite's *Database
file* is a `path_field` too since then, where `sqlite_fields` had carried a hand-copied
duplicate of its layout; its Browse went from tabindex 62 to 61 in the move, the order
unchanged.
**An *Authentication* picker appears only on an engine offering more than one mode** — SQL
Server, PostgreSQL and the MySQL family, through `AuthMode::offered`; SQLite has none — between Database and the credentials, at tabindex 68 in
the gap the surrounding indices were spaced to leave. `DraftSignals::auth` is threaded through
create, load, reset and `to_connection` like the rest. The list is read once, untracked, because
`server_fields` is rebuilt when the engine changes. What follows the dropdown is keyed on the
mode **in force**, not the stored one — a SQL Server connection switched to MySQL keeps
`AzureCli` in its draft and must still show a password — through one `in_force` closure over
`AuthMode::in_force`, the rule the driver asks. The credentials are a `dyn_container` keyed on
the mode in force's `(uses_user, uses_credentials)`, asked rather than matched, so the rows
exist exactly where the driver reads them: User and Password for a password, the User row alone
for a cloud IAM token — the server account it signs in as — and `nothing()` for Windows and
Entra, each its own child rather than hidden fields, for the reason `conn_form`'s
`engine_block` is built per engine — a hidden field stays in the Tab ring, so Tab would land on
a password nobody can see. **Every other mode's hint is not a credentials arm but the
picker's own third child** (`sign_in_hint`), under the dropdown at the stack's 6px as the
Database field's hint sits under its field. As a credentials arm it sat in the server column's
20px gap, over three times further from the dropdown, and read as a field of its own rather
than a note about the choice above it. Both containers are also styled
`widgets::collapse_unless` on the mode (the credentials' on `uses_user`), not only emptied: an empty `dyn_container` is still a
box, and the credentials' would claim the column's 20px below the picker. The picker's stack is
`width_full` so the hint wraps at the column. Windows names the account from
`USERDOMAIN`/`USERNAME` (*"Signs in as the Windows account Schemaic runs as (DOMAIN\user). The
server must know that account as a Windows login."*), and Entra says *"Signs in as whoever is
signed in to the Azure CLI (run `az login` first). The token is fetched when a connection needs
one and never saved."* The two cloud IAM modes say what the server side needs as well: AWS's
that the user needs the `rds_iam` role (PostgreSQL) or the `AWSAuthenticationPlugin` (MySQL),
Google's that the user is the IAM user or service account Cloud SQL knows.
**Under the Host/Port row, a SQL Server named instance says Port goes unused**
(`connection::named_instance_port_note`, read in `server_fields` from the draft's engine and
host): SQL Server Browser answers with the port, so a number left in the field is not what is
dialled, and the note says how to skip the Browser — the bare host and the instance's own port.
The label is **hidden**, not merely empty, when the note is `None`: an empty label still takes
a line and the column's gap.
**Its words are the catalogue's** (`connections-*`) — the title (through
`modal_title_owned`), the list's New/Import/Export rows and its right-click Duplicate/Delete,
the footer's Delete/Test/Save, the four section headings, every field label, toggle and hint,
the five file pickers' window titles (`connections-*-dialog`) and the *Browse…* inside a path
field (`connections-browse`); the failed Test's tooltip falls back to `connections-test-failed`.
`field` and `masked_field` take their label as `impl Into<String>`, `host_port_row` and
`path_field` as `String` — `path_field` clones the dialog title into its Browse closure — and
Save's flash face reads the translated label for its pinned width as well as its text. The
Windows sign-in hint is two whole messages, `connections-windows-hint-named` with `$account`
(`DOMAIN\user`) and `connections-windows-hint` without: `windows_sign_in_hint` reads the
environment and `windows_sign_in_hint_for(domain, user)` composes, which is what
`the_windows_sign_in_hint_reads_as_it_did_in_english` pins. The pickers' choices are drawn
here as properties of a typed value (*The string catalogue*) rather than passed through from
`connection`: `ssl_mode_label`/`ssl_mode_description` (`tls-*`, `tls-*-description`),
`ssh_auth_label` (`ssh-auth-*`) and `auth_mode_label` (`auth-*`, where Microsoft Entra, AWS
IAM, Google Cloud IAM and the CLIs named are product names), held to core's `label` and
`description` by `english_names_every_picker_choice_as_core_does`; the Environment and AI data
access pickers and the latter's hint take `i18n::environment_label`/`ai_data_label`/
`ai_data_hint`, which live there because the top bar and Settings call them too. The notes
under the fields are the same move for a sentence: core still decides *whether* one is said,
and the form maps the answer to its own words with `.map(|_| …)` — `verify_ca_caveat_words`
(`tls-verify-ca-checks-host`) for `SslMode::caveat`, `tls_floor_note` through
`tls_floor_words(auth)` (`tls-floor-azure-cli`/`-aws-iam`/`-gcp-iam`/`-other`) for
`AuthMode::tls_note`, and `connections-read-only-mssql` and `connections-named-instance-port`
inline for `read_only_caveat` and `named_instance_port_note` — held to core's text wherever
core says one by `english_says_every_field_note_as_core_does`. The Init SQL note's refusal is
the same move for a value: it used to show `init_sql::check`'s finished English whatever the
interface's language, and `init_refusal_text` now maps each `InitWhy` to its own
`connections-init-*` message inside `connections-init-refused`'s `$statement`/`$why` framing,
held to `InitRefusal`'s `Display` for every reason by
`english_says_every_init_refusal_as_core_does`. The statement's preview and the lower-cased
setting names it carries are SQL, and are not translated. The SSH host's note is the same move
again: `ssh_config_hint` used to show `ssh_config::describe_route`'s finished English, and
`route_note_text` now maps the `RouteNote` — `connections-route-config`, whose `$key` and
`$through` selectors take `some`/`none`, for the resolved line;
`connections-route-proxy-command`, `-match`, `-match-destination` and `-include` for an
unread route, about `connections-route-jump-host`'s phrase where the first jump host is the
one refused; `-config-jump-password` and `-jump-own-proxy-jump`; and
`connections-jump-host-error` around a `connections-jump-*` reason — held to the `Display`s for
every resolved combination, every unread route for both hosts, both other refusals and every
`JumpEntryError` by `english_says_every_route_note_as_core_does`. Hosts, users, ports, key
paths and jump lists pass through as the connection and the config spell them. What stays
English: the Type picker's engine names (`DbKind::label`), which are product names and are also
what is persisted as `Connection::db_type` and read by `default_port`/`is_networked`; the
samples `user@bastion:22, inner` and `SET search_path TO app, public`; and other modules'
sentences — `lib.rs`'s `TestLine::text`. `connection`'s `AuthMode::transport_refusal` never
reaches this form, and is worded where it does arrive, as a `DbRefusal` (`i18n::db_refusal_text`);
`external_client_refusal`, which never did either, is a `launch::Refusal` the app words now.
`init_refusal_text` is `pub(crate)` for that same function, which words a connect's init SQL
refusal (`DbRefusal::InitSql`) through it, so the field's note and the refused connect say it
alike. `SshTunnel::route`'s `JumpHostError` is worded by
`jump_host_error_text`, under the Jump hosts field and inside the SSH host's note alike.
