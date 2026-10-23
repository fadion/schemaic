# `core/connection.rs`

`connection.rs` — the saved-connection model. A `Connection` is a database **server**, not one
database — the sidebar lists all of its databases. **SQLite is the exception to that whole
sentence**: there is no server, so `Connection::file` is the entire target and
`host`/`port`/`user`/`password`/`ssh` are inert, which in turn means no secret reaches the
keyring (an empty password is *deleted* from the store, not written), no tunnel is opened, and
the connection has exactly one database — the one SQLite calls `main`. `file` is
`#[serde(default)]` so every connection saved before it loads unchanged, and it sits *beside*
the server coordinates rather than replacing them because a connection's engine is editable in
place: switching to SQLite and back must not discard the host it is going back to. Three
consequences are written down where they are easy to get wrong — `endpoint()` shows the file's
*name* (`host:port` would read `:0`, which looks like a misconfiguration), the split handles
**both** platforms' separators since `connections.json` is portable and `std::path` on Linux
returns a whole Windows path as its own file name, and `targets_same_server` counts the file,
because pointing a connection at another `.db` reaches an entirely different set of tables and
is reached the same way a repointed host is. `is_sqlite`/`is_postgres`/`is_mssql` are the one
answer to which engine a label names — `schemaic_db::Engine::from_db_type`, the form's picker and
`SqlDialect::from_db_type` all delegate, the last of which used to re-spell the aliases itself.
`is_mssql` takes `SQL Server`/`sqlserver`/`mssql` as **whole labels only**, never a substring:
`MySQL` contains "SQL" too, and a label that meant SQL Server to the driver and MySQL to the form
would send a TDS handshake to a MySQL port. `read_only_caveat` is the sentence the form puts
under the Read-only switch on SQL Server alone, since that engine's read-only is a rollback
rather than a refusal (`db::mssql`).
**`sql_server_address` is the one reading of a SQL Server Host field**, as Microsoft's clients
read it: a leading `tcp:` (any case) is the protocol and goes, `.` and `(local)` are
`localhost`, a trailing `,port` is the port — a non-zero `u16`, or the text stays in `server`
for DNS to refuse in its own words — and `server\INSTANCE` is a named instance, both halves
non-empty and the instance holding no second `\`. **An inline port wins**: the client dials it
and asks no Browser, so `instance` is `None` once a port is written. The same text used to be
read three ways (S5-L1-02): `db\SQLEXPRESS,1500`, the SSMS/ADO.NET spelling, had the connect
ask SQL Server Browser for an instance literally named `SQLEXPRESS,1500`, `sqlcmd` refuse it,
and the importer dial `db:1500`; and `(local)\X` and `tcp:db\X` reached DNS verbatim when typed
(os error 11001) while the importer mapped them
(`a_sql_server_host_is_read_as_microsofts_clients_read_it`). `sql_server_instance` is that
reading's named instance alone, and `SqlServerAddress::dialled(saved)` its TCP target — the
server and the inline port, else the saved one, and `None` for a named instance. A DNS name or
an address holds no `\`, so nothing else is read as an instance (`instance_tests`). Every place
that takes a saved host apart asks it: the connect, which asks SQL Server Browser for a named
instance's port and so leaves the Port field unused, and otherwise dials `dialled`
(`db::mssql`); `launch::sqlcmd_args`, whose `sqlcmd` does the same lookup itself; and
`Connection::tcp_target`, which is what an SSH tunnel is opened to and what `endpoint()` shows —
`dialled` on SQL Server, the fields as saved on any other engine, and **the host as saved for a
named instance**, which has no TCP target of its own, so that `ssh::open_tunnel` still sees the
`\` and refuses it. The importers keep their own parsers for their sources' grammars and write
this spelling back, and the two readings are pinned to agree rather than shared
(`a_typed_host_and_an_imported_one_agree`, over `db\SQLEXPRESS,1500`, `db,1500`,
`(local)\SQLEXPRESS`, `tcp:db\X`, `lpc:db` and `.`) — `lpc:` stripped as the importer strips
it. Two more readers take the server from it rather than the raw field: a tunnelled
connection's TLS name to verify (`Db::connect`'s `hostname_override`, from `tcp_target`), and
`mssql::is_azure_sql`, so the Azure portal's own `tcp:srv.database.windows.net,1433` gets the
Azure login bound (`a_tunnel_moves_the_address_and_keeps_the_name_to_verify`,
`an_azure_sql_host_is_told_by_its_name`).
`named_instance_port_note` is the sentence the form puts under Host and Port when the host
names one, on SQL Server alone — *"A named instance: SQL Server Browser gives its port, so Port
is not used. To skip the Browser, set the host without the instance and the port it listens
on."* — so it does not show for `db\X,1500`, whose port is used; and `endpoint()` follows it,
showing `server\INSTANCE` with no port, since the saved one is not what is dialled; on another
engine a `\` is no instance and the host shows as typed (`a_named_instance_endpoint_has_no_port`).
`same_engine` is that pair asked of *two* labels — `MariaDB` and `MySQL` name one engine, as do
`pg` and `PostgreSQL`, and as do `MySQL` and the empty label that predates the field — so the
question is not a string comparison. It is what the connection form's Type picker tells its own
change apart from a load with.
**The registry itself had no accessor at all**, and `by_id`/`read_only_of`/`label_of` are it.
`connections: RwSignal<Vec<Connection>>` is one of the app's two central registries, and every
consumer reached into the `Vec` — sixty-one hand-written `find(|c| c.id == …)` across two crates,
with nothing to grep for and nothing to change in one place. `read_only_of` is the question most
of those scans were asking, and it had **seven** spellings: two byte-identical ten-line memos
3,100 lines apart in `ui/lib.rs` (the second's comment, *"Same derivation as `center`"*, an
acknowledgement of the copy rather than a link to it), a module-private `conn_read_only` in
`overlays.rs` nobody else could find, and four more — and two of the seven feed a *write gate*
rather than a dimmed button, `ddl_preview` handing its answer straight to `widgets::accept_launch`
and `compare_view` to `preview_of_plan`. **An unknown id is writable, and that is now a decision
instead of a coincidence**: all seven defaulted that way, five as `is_some_and` and two as
`map(…).unwrap_or(false)`, with no single site at which anyone could review it. It is right
today — the ids come from tabs, so an absent one means the connection was deleted while something
still referred to it, and the write that follows fails on the missing connection rather than on
this flag — and `an_unknown_connection_is_writable_deliberately` is the test the next person to
change it has to change with it. `label_of` is the same shape for a name, where the *fallback* is
the decision: a plan, a comparison or a preview that has lost its connection still has to name
what it is about, and `connection 7` is a worse name than the one the user gave it and a far
better one than a title reading *"Apply to  "*.
**`cli_access` is who may *reach* this connection, where `read_only` is what may be *done* over
it** — two fields rather than one setting, because a connection can be exposed to the headless
CLI and still refuse writes, and usually should be. It is `#[serde(default)]` and off, so every
connection already on disk reads as not exposed: the CLI runs with nobody at the keyboard, and an
upgrade that defaulted it on would hand an agent the production connection the user had already
saved, silently (`a_connection_saved_before_the_cli_existed_is_not_exposed_to_it`, with
`the_cli_access_flag_round_trips` for the direction that would break by never persisting). Only
the GUI writes it — the CLI reads `connections.json` and never writes it, since the app may be
running and owns that file — and `cli/select.rs`'s `select` is the one place a connection is
*refused* on it, its `listed` and `run`'s `list --all` column being the two that merely read it.
**`folder` is a name, not an id into a folder table**: nothing else describes a folder, so one
exists exactly while some connection names it, and there is nothing to create, rename or delete
on its own. `""` is none, and it is `#[serde(default)]`, so a file written before folders loads
ungrouped (`a_connection_saved_before_folders_has_none`) with no migration beyond that; there is
no collapse state either, a heading in a dropdown needing none. `group_by_folder` is the one
ordering, because two lists draw it — the header's `conn_menu_overlay` and Manage Connections —
and two orderings would disagree: the ungrouped connections first under no heading, which is the
list exactly as it was before folders, then each folder A–Z ignoring case, each group in saved
order. A folder is its name **as read**: trimmed, a blank one is none, and names differing only
in case are one folder headed by the spelling met first, since two folders a user cannot tell
apart on screen would be a place to lose a connection
(`a_folder_name_is_trimmed_and_matched_ignoring_case`). `folder_names` is those headings without
the ungrouped `""` — what the connection form's Folder field suggests — so accepting one, which
writes the list's spelling (`FieldCfg::suggest`), files the connection under the folder already
on screen rather than a second casing of it
(`the_folder_suggestions_are_each_folder_once_as_the_lists_spell_it`). `listed_rows` is that as the rows a list
draws — a `ListedRow::Folder` heading over its `ListedRow::Connection(id)`s, and no folders at
all is the saved order with no headings — and `listed_ids` is the ids in drawn order, which is
what the arrow keys walk: stepping in saved order would jump between folders on screen
(`a_listing_heads_each_folder_and_the_arrows_walk_it_as_drawn`). `first_listed` is its top row,
where a delete lands — form, fallback active connection and switch alike; they took the first
*saved* one, which folders put under any heading. The command palette's Switch
Connection, `schemaic list` and the MCP server do not group; they stay flat.
`AiData` is the connection's **AI data-access level** — `SchemaOnly` / `OnRequest` (the
default) / `Full` — and the single gate over every path that can carry this connection's rows
off the machine: the `run_query` tool, `describe_table`'s sample rows, the grid's
attach-to-chat, and the value sampling behind AI Summary / Fill / Seed. Per connection because
a local scratch database and a client's production server are not the same risk, and one global
answer forces the careless setting on one of them. It is `Option<AiData>` on `Connection`:
`None` means "saved before the setting existed", which `migrated_ai_data` settles once at
startup from the old global `ai_run_queries` flag, so an upgrade neither grants access nobody
chose nor withdraws access that was working. What counts as *evidence* of that old flag is
`persist::legacy_ai_run_queries_in(&[u8])` — split out of the `std::fs::read` because the
decision is the whole point and the I/O put it out of reach. Four answers, three of them
`None`: unparsable JSON, no such key, and a key holding something that is not a boolean. Only a
recorded boolean is evidence, because what it decides is a one-way promotion of every saved
connection to `AiData::Full`, and the migration never re-resolves. It is the app's to run:
`schemaic mcp` cannot read the old flag, so there `None` is the default level
(`cli/mcp.rs`'s entry). There is deliberately **no masking option** — a
model cannot tell a masked value from a real one, so it reasons confidently about fiction, and
the questions where values matter are exactly the ones masking ruins.
**The words on these types are not what the interface shows.** `Environment::label`, `AiData`'s
`label`/`hint`, `SslMode`'s `label`/`description`, `SshAuth::label` and `AuthMode::label` stay
English and are the wording the interface's own is held to — `ui/i18n.rs` and
`connection_form.rs` draw each from the catalogue (*The string catalogue*) — and
`Environment::badge_label` is asked by the top bar only for *whether* there is a badge. So,
now, are the notes this module writes under the form's fields — `SslMode::caveat`,
`AuthMode::tls_note`, `read_only_caveat`, `named_instance_port_note`: the form keeps the
decision here and words the answer itself (under `connection_form.rs`). So, now, is
`AuthMode::transport_refusal`, which arrives as a database error from `schemaic-db`: it used to
reach the user as it was, English, and is `short.then_some(self.transport_sentence())` now —
`transport_sentence` holding the four sentences, one per cloud sign-in and one for the rest —
while `schemaic-db` carries the refusal as `DbRefusal::Transport(AuthMode)` and the window words
it from the mode (`i18n::db_refusal_text`). `external_client_refusal` says nothing itself any more — it answers
`launch::Refusal::CloudToken`, one of `launch::*`'s own refusals, which the app words (under
`launch.rs`).
**`statement_timeout_secs` is this connection's statement timeout over the app's** (*Settings →
Query*, `persist::statement_timeout`) — the field's own example is a reporting replica that
legitimately runs for an hour beside a production primary that never should. It is
`Option<u64>` and `#[serde(default)]`, and the two non-values must not collapse: `None` follows
the app, which is what every connection saved before the field does
(`a_connection_saved_without_a_timeout_follows_the_app`), while `Some(0)` is **no timeout on
this connection whatever the app says** — read as "follow the app", a connection that turned
its timeout off would be held to the app's again
(`a_connections_own_timeout_wins_and_zero_means_none_here`). `Connection::statement_timeout(app_secs)`
resolves it to a `StatementTimeout { secs, per_connection }`, the flag being what lets the
timed-out message name the setting that fired (`StatementTimeout::cancelled_message`, the
English an error's `text` carries, and the app's `timeout_message`, its wording, held equal by
`english_says_the_timed_out_notice_as_core_does`; the statement timeout's passage under
`ui/lib.rs`); `statement_timeout_of(list, id, app_secs)` is the registry's
accessor, and an unknown id follows the app for `read_only_of`'s reason — the run that asked
fails on the missing connection, not on this
(`a_timeout_is_looked_up_by_id_and_an_unknown_one_follows_the_app`).
`statement_timeout_choice_label` is the form's English: *Same as the app* for `None`, otherwise
`persist::statement_timeout_label`, so a message quoting the value matches either dropdown
(`the_forms_timeout_labels_follow_the_app_or_say_its_words`). The form itself words the choice
through `ui::i18n::statement_timeout` and is held to this (`connection_form.rs`'s entry), so the
function now stands as the English the interface is pinned to. It is listed in the redacting
`Debug`, and `duplicate`'s struct update carries it.
**`init_sql` is the session settings every connection opened for this one runs first** —
`SET search_path …`, `SET time_zone …`, `SET ROLE …` — and what may be in it is
`core::init_sql`'s decision, asked at every connect rather than on save: the form says what
`check` makes of the text as it is typed, and Save does not refuse it, the connect does. `""` is
none, and it is `#[serde(default)]`, so a connection saved before the field runs nothing
(`a_connection_saved_before_init_sql_runs_none`). It is no secret, so the redacting `Debug`
prints it, and it travels in a connection export and import through serde as `folder` does. It
is a term of `invalidates_open_connections` (not of `targets_same_server`: the server is the
same), so saving a changed one releases a Manual tab's pinned `Session` — set up by the old
text, it would otherwise go on running under the old `search_path` or role while every fresh
operation ran under the new (`an_init_sql_edit_invalidates_what_is_open`). **But not unasked
when its transaction is open**: that release used to be silent, and since the socket and the
transaction were both still alive, the release was itself what rolled the work back — while the
write log kept those writes as `ok` with no end line. `open_session_after` answers
`OpenSession::AskFirst` for such an edit, and the app puts each open transaction through the
Commit / Roll back / Cancel prompt before re-pinning
(`only_a_torn_down_tunnel_drops_an_open_session_unasked`; the `app/main.rs` passage on what an
edit invalidates).
`SshTunnel`/`SshAuth` cover the tunnel's own
auth, including `Agent` (delegates to the running SSH agent, storing no secret at all).
**`SshTunnel::jump` is the route before the SSH host**, in OpenSSH's `ProxyJump` spelling —
`[ssh://][user@]host[:port]`, comma-separated, nearest first — and `route()` reads it into
`SshHop`s: the jump hosts, then `SshTunnel::host` itself, which is the list `ssh::open_tunnel`
walks. An entry naming no user takes the tunnel's and one naming no port takes 22, as `ssh -J`
does; an IPv6 host is bracketed, and the user is split at the **last** `@`, so an email-shaped
one keeps its own (`jump_hosts_come_first_nearest_first_with_the_tunnels_user_and_port_22`).
**An entry it cannot read refuses the whole route** — empty from a stray comma, a space, no
host, a bad or zero port, an unclosed `[` — with *Jump host N: …*, because a hop dropped
silently would connect by a route the user did not write
(`a_jump_entry_that_cannot_be_read_refuses_the_route`); the form prints that same sentence
under the field as it is typed. The refusal is a value, `JumpHostError` — the entry's 1-based
`number` and a `JumpEntryError` (`Empty`, `HoldsSpace`, `UnclosedBracket`, `TextAfterBracket`,
`NoHost`, `NotAPort`, each but the first carrying the wrong part as typed), which
`parse_jump_host` answers — so the SSH host's note can word it; its `Display` is the English,
`JumpEntryError::message` the reason after the colon. Every hop signs in with the tunnel's own method and
credentials — one password, one key or one agent for the whole route — so a jump host needing
a different sign-in from the SSH host cannot be described. Under password sign-in that holds
only for hops the connection names: a jump host only `~/.ssh/config`'s `ProxyJump` supplies is
refused rather than sent the password (`ssh_config::may_offer_password`). `jump` is no secret, so the
redacting `Debug` lists it; `trimmed` trims it; and it is one of `targets_same_server`'s terms,
since another route reaches the server through machines the open tunnel never touched — saving
a changed one evicts the cached tunnel through `save_conn`'s existing `repointed` path
(`every_ssh_field_has_been_judged` is the tripwire that made it a decision). `conn_import`
writes it empty on purpose; see there. What `open_tunnel` walks is the route of the tunnel
*after* `~/.ssh/config` has resolved its host as an alias and filled what it left open, the
jump hosts included (`ssh_config::resolve_route`); the saved fields, and so `targets_same_server`, keep the
alias as typed.
`ConnStatus::is_down` treats `Unknown` (not yet checked, or a tunnel still coming up) as
*non*-blocking — only a confirmed failure gates work.
**`LoadFailure` and `TunnelFailures` are what an SSH connection with no tunnel says, in place of
one sentence for every reason.** The app opens a tunnel to keep in one place, `load_schema`'s
background task, and keeps it only if `list_databases` through it succeeds as well — so a load
that failed at either step left the connection with no tunnel, and its error went to
`tracing::error!` and nowhere visible. Every later `db_for` and the terminal's `open_db_cli` then
answered "SSH tunnel is not established yet", for good, over a tunnel that had in fact been
refused, the sentence masking the one cause worth reading; it was reported from macOS, where the
tunnel evidently never opened. `LoadFailure` says which step failed — `Tunnel { text, shown }`
(connect, host key, key file, agent, authentication) or `Server { text, shown }` (the tunnel
opened, the server behind it failed the load, and the tunnel closed with it), each its
`DbError` as `to_string()` and as the app's `db_error_shown` — and is the error half of the
app's `ConnectResult`, which was a `String`. `TunnelFailures` holds each connection's last one, and `refusal` is the sentence:
the old one while nothing is recorded, otherwise the reason and *Refresh the schema to try
again* — the only retry there is, since nothing re-opens a tunnel on its own. `refusal_of` is
the same answer as a `TunnelRefusal` (the reason borrowed, its closing full stop off), which the
app says through `ui::i18n::tunnel_refusal` (`tunnel-*`); `refusal` is its English. A
failure is both there too, `TunnelRefusal::Tunnel { why, shown }` and `Server { why, shown }`:
`refusal` reads `why` and `tunnel_refusal` reads `shown`, so the window says the failure as it
says it everywhere else — a tunnel's, an `SshFailure` (under `core::model`), included — and
core's sentence keeps the English (`a_server_failure_is_refused_in_english_and_shown_as_worded`;
`the_tunnel_refusal_says_the_servers_failure_as_shown` in `ui::i18n`, which reads a `Tunnel`'s
`shown` too — the app's tunnel arm of `load_schema` fills it with `db_error_shown(&e)` as the
server arm does). `Server` is kept
apart because calling it a tunnel failure would send the user to fix SSH settings that work
(`a_server_failure_behind_the_tunnel_is_not_called_a_tunnel_failure`, in `tunnel_failure_tests`).
The app's bookkeeping is the half the type cannot pin: `load_schema` clears the entry when it is
about to open a fresh tunnel (no cached port) and when a load lands a new handle, so a retry under
way reads as still opening rather than as the last attempt's error; and it records a failure
whatever the landing — a load the user has moved on from still left *that* connection without a
tunnel — but only when none is cached, since a reload that failed on the server over a live
tunnel keeps it, and the operations that find it need no excuse. Manage Connections' Test is
still the other place the SSH error is shown in full. `SshAuth`/`Environment`/`AiData`
deserialize through a `…Raw` shim with `#[serde(other)]`, so a value written by a newer build
degrades to a default instead of failing all of `connections.json` — and for `AiData` that
default is deliberately the *safe* level, since an unknown one written by a newer build means
more access than this one understands. There is deliberately no
`mysql://user:pass@host` builder, and the password fields here aren't what's on disk — see
`secrets.rs`. **`Debug` is hand-written on both `Connection` and `SshTunnel`** and redacts the
three secrets they hold through the private `Redacted` wrapper; the reasoning, and why the impls
list their fields one at a time, is under *Connection identity* in the invariants. `duplicate` is the copy the connection list's right-click menu makes — a
**struct update**, so a field added to `Connection` later is carried by construction; the
failure mode is a credential silently not copied, which the field-by-field form would not
fail to compile over. `targets_same_server` is the "is this still the same server" test the schema
tree's reload gates on (see `schema.rs`'s `SchemaState::begin_refresh`) — everything that
decides which server the next query reaches and nothing else, so a rename or a colour can't
blank the tree and a repointed host can't leave another server's databases on it. **The sign-in
mode in force (`effective_auth`) is one of those things**, beside `user`: Windows or Entra signs
in as somebody the SQL login was not, who may see other databases, and an MCP subprocess or a
pinned Manual session left running as the old account would be the TLS lie below in another
shape (`a_new_sign_in_mode_is_a_different_login`). The connection form's Test result resets on
it for the same reason.
**`invalidates_open_connections` is the wider question beside it, and it is wider by a socket
that is not encrypted.** *Which server does the next query reach* is the tree's question; *is
anything I have already opened still valid* is `save_conn`'s, and the two part company on the
transport. Save a MySQL connection with TLS `Disable`, pin a tab to Manual so `open_session`
builds a `Db` from the connection as it then stands, then set the mode to `Require` and press
Save: every one of `targets_same_server`'s eleven fields is unchanged, so nothing was torn down —
the pinned `Session` kept running and every statement in that tab, its `COMMIT` included, went on
travelling in the clear while the form, the status bar and every fresh operation reported TLS.
So this adds the TLS mode and its three paths, in **both** directions (turning it off leaves a
session negotiating it, the same lie the other way), plus `ssh.auth` and `ssh.key_path`, which
decide what the tunnel authenticates as, and `init_sql`, which decides what the session was set
up as. The password stays out for `targets_same_server`'s
reason: a corrected one reaches the same server over the same socket, and an already
authenticated session is not made wrong by it.
**`open_session_after` splits that yes by whether the socket is still up.** The private
`moves_transport` holds the server and TLS/SSH terms, and `invalidates_open_connections` is it
*or* a changed `init_sql`. `open_session_after` returns `OpenSession::Dropped` only where the
save tears the tunnel down — a tunnelled connection repointed, whose tunnel `save_conn` evicts
with the forwarded socket in it — and `AskFirst` for every other stale session: a TLS, CA,
client-key or SSH-credential edit (which keeps the tunnel), a repointed direct connection, an
init SQL edit, and **a flipped `read_only`, either way**, since the pinned session's `Enforce` was
fixed when it opened: ticked, `run_verdict`'s text gate passes a `SELECT` whose function writes
(R1-L2-01); lifted, the stricter session was once `Kept` and refused every write (*cannot
execute UPDATE in a read-only transaction*) while the status bar said *Write mode*, until the
tab left Manual — found driving the 8dd78136 hand checks. The old split asked only after
an init SQL edit, on the premise that a transport edit had already lost the transaction; it held
only for the evicted tunnel, and everywhere else the unasked release was what rolled the work
back (`only_a_torn_down_tunnel_drops_an_open_session_unasked`,
`ticking_read_only_does_not_keep_a_writable_session`). **`OpenSession::pinned_fate(TxAsked)` is
each tab's answer**: `Keep`, `Release` (to Auto, `Dropped` only), `Repin` — the tab stays in
Manual on a session opened under the connection as saved, at once with nothing open or after
Commit / Roll back — or, after Cancel, `RepinWhenTxEnds`: the work is kept, the stale session
only until the transaction ends (`a_cancelled_question_repins_when_the_transaction_ends`; it
used to stay for the tab's life, S3.1-L5-01).
**`ticks_read_only(after)` is the one edit asked *before* it applies, so `RepinWhenTxEnds` is
never its answer.** Asked after, as every other stale-session edit still is, a Cancel kept the
old **writable** session for the rest of its transaction on a connection the whole UI called
read-only, with nothing but `run_verdict`'s text gate in front of it (security review S20). The
app now settles every open Manual transaction on the connection first and applies the flag only
once all are settled, and a Cancel leaves the connection writable — so every session on a
read-only connection is server-enforced, with no window. It is `!self.read_only &&
after.read_only` and nothing wider: lifting the flag, or an `init_sql` edit, is not this question
(`only_ticking_read_only_asks_before_it_applies`).
**`Tls` and `SslMode` are the transport half**, and they live here rather than in
`schemaic-db` for the reason the module exists: this is the saved connection, and the mode is a
saved field. Five rungs — `Disable` / `Prefer` / `Require` / `VerifyCa` / `VerifyFull` — asked
through four predicates (`negotiates_tls`, `requires_tls`, `verifies_certificate`,
`verifies_hostname`) rather than matched at a call site, plus `stronger_of`, which is the rule
for merging two descriptions of one connection and the direction an *import* must move
(`conn_import` had a DBeaver SSL block silently clamping a URL's `verify-full` down to
`require`), and `protects_more_than`, the **strict** half of the same ladder. `stronger_of`
answers *which* mode wins and says nothing about whether there was a contest, and a merge needs
both: the winning mode, and whether the losing row's TLS material should travel with it — see
`conn_import`'s `absorb`. It lives on the type for `rung`'s reason, a caller spelling the
comparison out being the variant-matching this type exists to prevent, and ties are the case that
matters: a merge gated on it must let a tie through, which a `stronger_of(b) == self` would not. `Tls::plan` collapses the five into the four decisions a handshake is made of —
`TlsPlan` — which is what `schemaic_db::tls` translates for the two drivers; `Prefer` alone
carries `fallback_to_plaintext`, and `hostname_override` is what keeps a **tunnelled**
`verify-full` checking the far end's certificate instead of `127.0.0.1`. The `SslModeRaw` shim
is the one `#[serde(other)]` on this type that does **not** degrade to `default()`: an unknown
mode resolves to `STRICTEST`, because the default is `Disable` and a Velopack rollback reading a
newer build's file would otherwise turn an encrypted connection into a plaintext one, silently,
and then persist that on the next save. `caveat` is the one place a rung's words and its effect
are allowed to differ per engine, and it carries exactly one sentence today — see
`db/tls.rs` for the driver defect behind it. **The connection's `database`** is the other field
the same range added: a *default*, never an override, resolved by `Connection::default_database`
and honoured by `schema::first_bindable` (which is what makes a tab open where the form says it
will) — and deliberately not the same thing as "no database at all", which `schemaic_db`'s
`Scope::Server` spells separately.
**`AuthMode` is how a connection signs in**, and `Connection::auth` holds it:
`Password` — every engine's, and the default — `Windows` and `AzureCli`, the last two SQL
Server's. Neither of those reads the form's user or password (`uses_credentials`): Windows
presents the identity the app runs as, through SSPI, so there is nothing to store, and
`AzureCli` hands the server a Microsoft Entra token from the Azure CLI's own sign-in, fetched
when a connection needs one and never saved (`core/entra.rs`, `db/entra.rs`). **PostgreSQL and
the MySQL family offer two more, the clouds' IAM sign-in** — `AwsIam`, an RDS token from the
AWS CLI, and `GcpIam`, a Cloud SQL one from the Google Cloud CLI, what RDS and Cloud SQL run —
while SQLite, a file, offers the password alone (`postgres_and_mysql_offer_cloud_iam_sign_in`,
which replaced the assertion that every engine but SQL Server offered nothing else). These two
sit between the others: they read the form's **user** and never its password (`uses_user`
true, `uses_credentials` false — `a_cloud_iam_mode_reads_the_user_and_not_the_password`),
because the token signs in as a server account the user names, and it is sent where the
password would go. `cloud()` names the CLI that mints it (`core/cloud_iam.rs`,
`db/cloud_iam.rs`). **`external_client_refusal` is what that costs**: `psql`, `mysql`,
`pg_dump` and `mysqldump` are handed the connection's password in their environment, and an
IAM connection has none — the token is fetched by the driver, inside the app — so *Open in CLI*,
*Back up* and *Restore* refuse one with its `launch::Refusal` (under `app/main.rs`) and `sqlcmd_args`
has an arm for the two that refuses with it too. `offered(db_type)`
is the one list, in picker order with the default first, and it is where the OS enters:
Windows sign-in is offered in a Windows build alone, a process anywhere else having no Windows
identity to present. **The mode a driver sees is `effective_auth`, asked through the engine as
`default_database` is** — the picker is editable in place, so a SQL Server connection switched
to MySQL keeps its mode with no control left that could unset it, and a `connections.json`
carried to a Linux machine holds a Windows mode that build cannot honour; both sign in with the
password (`a_mode_the_engine_does_not_offer_signs_in_with_the_password`). **The rule itself is
`AuthMode::in_force(db_type)`, stated once** and asked by `effective_auth`, by `Db::with_auth`
for the MCP handoff and by the form's sign-in rows: each used to write it out, only the first
was tested, and a change to one would have had the form hide the password fields while the
driver still sent one, or the subprocess sign in unlike the app
(`the_mode_in_force_is_an_offered_one_or_the_password`,
`a_handoff_mode_the_engine_does_not_offer_signs_in_with_the_password`). The field is
`#[serde(default)]` and deserializes through `AuthModeRaw`, the same `#[serde(other)]` shim as
the rest, degrading to `Password` — the password is what the file still holds, and the server
says so plainly if it is not enough (`an_absent_or_unknown_auth_mode_is_a_password`). A user and
password are kept under a mode that does not send them — the form's `to_connection` writes both
whatever the mode — so switching back to a password finds them where they were.
**A sign-in has a TLS floor, and Entra's is `VerifyFull`, raised rather than asked for.** An
Entra access token is not a password for this server but a bearer token for
`https://database.windows.net/`, so for its hour of life it opens every Azure SQL database the
account can reach — and it used to travel over whatever the picker said. A hand-made
connection defaults to `Disable`, which on SQL Server is TDS `Off` with `AnyCertificate`
(`db/tls.rs`), so anyone on the path presenting any certificate received the token in the
login packet. `AuthMode::tls_floor` is `Disable` for `Password` and `Windows`, `VerifyFull` for
`AzureCli` and the clouds' below, and **`Connection::tls_mode_in_force` — the picked mode `stronger_of` the floor of
`effective_auth` — is what `uses_tls` and `tls_plan` read**, so the plan `Db::connect` builds,
and the one the MCP endpoint handoff serialises off `Db::tls_plan`, are raised, with the
connection's own CA path and client identity (`an_entra_sign_in_always_plans_a_verifying_handshake`).
Reading the stored mode at either of those two is the bug back. What the floor does not cover
is a SQL login's password under `Prefer` or `Require`, which has the same exposure and stays the
user's call: for the token that call costs far more and buys nothing, since Azure SQL presents
a publicly issued certificate `VerifyFull` accepts with no CA file, and a named CA still
anchors it for a server that does not. A floor rather than a refusal the user must answer, because raising the
mode can only fail loudly. The stored mode is never written — see the form's `tls_fields` for
why — so `tls_note` is the sentence that keeps the picked and connected modes from being two
things silently. `TlsPlan::verifies_server` is the rung read back off a plan (no plaintext
retry, no invalid certificate, the name checked), and `transport_refusal` compares a plan with
the floor for the driver's own check, which `mssql.rs` makes before any token is fetched
(`an_entra_token_is_refused_over_a_transport_that_verifies_nothing`), and `Db::sign_in_secret`
makes for the clouds (`an_iam_sign_in_without_tls_is_refused_before_any_token_is_asked`).
**The two clouds' floors follow what their tokens open**
(`the_cloud_floors_follow_what_their_tokens_open`). An RDS token is a password for one host,
port and user and lives fifteen minutes, so `AwsIam`'s floor is `Require` — an encrypted
session, which on MySQL is also what keeps the `mysql_clear_password` plugin's clear text off
the wire (`Db::opts_with_tls`). Verifying the server needs AWS's RDS CA bundle, which no system
store holds, so a verifying rung stays the user's call and the note says Verify full needs the
bundle as Root CA. The trade is stated rather than hidden: under `Require` anyone on the path
who presents a certificate receives the token, good for that one server for what is left of its
fifteen minutes. Google's is Entra's exposure — an OAuth access token valid an hour for every
Cloud SQL instance the account reaches — so `GcpIam`'s floor verifies the certificate, at
`VerifyCa` rather than `VerifyFull` because a Cloud SQL certificate names the instance, not the
host; its note and refusal ask for the instance's server CA as Root CA.
**`Connection::trimmed` is the padding rule, and it is one answer because the form had made it
seven times and omitted it five.** `to_connection` trimmed `port`, `file`, `database`, the SSH
port and all three TLS paths and stored `host`, `user`, `ssh.host`, `ssh.user` and
`ssh.key_path` verbatim — three lines apart, so the intent was established and those five were
the oversight, `ssh.key_path` being a file path sitting between two groups of trimmed file
paths. A hostname copied out of a hosting console or a wrapped email arrives with a trailing
space, a trailing space is invisible in a text field, and neither `sanitized()` nor
`schemaic_db` trims anything, so every operation on the connection failed on name resolution for
a host that looks correct on screen — which Manage Connections' Test renders as a bare red ✗
with the message discarded. The same shape for the user (an auth failure) and for the key path,
where the stray space is a file-not-found for a path the file picker may itself have produced.
**The three secrets are deliberately not trimmed**, and that is asserted as hard as the rest: a
leading or trailing space is a legal character in a password, an SSH password and a key
passphrase, and mangling a credential the user cannot see is the worse failure of the two;
`name` is left alone too, being a label rather than a coordinate. `ssh.jump` is trimmed as a
whole and `route()` trims each comma-separated entry, so only a space *inside* an entry is
refused. `folder` is a label as well and
*is* trimmed, because it is one that is matched: `Prod ` and `Prod` would be one folder on screen
and two in the file. `init_sql` is trimmed at its two ends and nowhere else — the lines and
indentation between its statements are left as typed (`init_sql_is_trimmed_only_at_its_ends`).
`conn_import` trims a host of
its own, so a connection *imported* from DBeaver got one and a connection *typed* did not —
routing that path through here as well is a separate change and has not been made. The defect is
a composition, so `trimmed`'s own unit test cannot see it: the call site reads a couple of dozen signals
and needs a Floem scope, which is why the second half is the source gate
`the_connection_form_trims_through_core` (under `ui/lib.rs`).
