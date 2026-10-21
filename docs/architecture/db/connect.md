# Connecting — `db/ssh.rs`, PostgreSQL's maintenance database, init SQL

SSH tunnels return a `TunnelHandle` (drop → port freed) with
keepalives + TOFU host-key verification (`ssh_known_hosts.json`). **`russh` stays on 0.62
deliberately**: 0.63 changes `Handler::check_server_key` to take a `&PublicKeyOrCertificate`
rather than a `&ssh_key::PublicKey`, so `ssh.rs` would have to decide what a *certificate* offer
means to a fingerprint store before it has anything to hash. That is a migration on the code that
decides whether to trust a host, not a version bump, and it is not being done inside a dependency
sweep.
**Every failure `ssh.rs` composes is a `core::model::SshFailure`** inside
`DbError::ConnectFailed`, built by the private `ssh_failed` — sign-in, key, agent, jump host,
connect, bind, the route, the named instance and the host-key refusals — so the window words the
sentence and the SSH library's error inside passes through; the one bare `Connect` left is the
listener's `local_addr` failure, the system's text alone. `tunnel_target_refusal` and the
host-key `refusal_message` answer `Option<SshFailure>`, and `TunnelClient::refusal` holds one,
so the verdict `open_tunnel` reports over russh's `UnknownKey` is that value rather than its
paragraphs. Their tests compare the value or its `Display`, which is the old English. In the
catalogue the two refusals are a message per paragraph, joined with line breaks by
`i18n::connect_failure_text` since a message holds none; the changed key's out-of-band check
and the file that re-trusts stay **one** message, `db-ssh-host-key-changed-advice`, so the order
`a_mismatch_says_the_key_changed_and_carries_both_fingerprints` pins in English — verify first,
then the file — is held inside a message a translator reads whole rather than across two the
code joins. **`Offered  { $key }` has two spaces on purpose**, to
line the fingerprint up under *Expected*'s, as core's English does; removing one is how
`english_says_every_connect_failure_as_core_does` was taken red.
**`open_tunnel` bounds its own handshake, for every caller**, because russh bounds none of it:
the default `inactivity_timeout` is `None`, `read_ssh_id` waits for a banner forever, and the
keepalives above only start once a session exists. So an endpoint that accepts TCP and never
speaks — a port-forward to a dead backend — held the GUI's connect spinner indefinitely, and only
the CLI, whose `run.rs` wraps the call in `--timeout`, ever gave up. There are two bounds, not
one, and the difference is deliberate: `TUNNEL_CONNECT_TIMEOUT` (20 s) covers TCP connect, banner
and key exchange, while `TUNNEL_AUTH_TIMEOUT` (60 s) is longer because an agent may be waiting on
a *person* — a 1Password prompt, Pageant's confirmation, a hardware key's touch — and that wait is
theirs, not the network's. Both go through the private `within`, which turns an elapsed bound into
`SshFailure::NoAnswer { phase, host_port, secs }` — *SSH {phase} with {host:port}: no answer
within Ns*, the phase an `SshPhase` the caller passes — and passes the phase's own
result through untouched otherwise — **its own error included**, so a host-key refusal or a wrong
key still names itself rather than reading as a timeout
(`an_answer_inside_the_bound_is_passed_through`, beside
`a_silent_ssh_server_is_given_up_on_at_the_bound`; both on tokio's paused clock, which is why the
crate's dev-dependencies enable `test-util`). The bound sits inside the function rather than at
the three call sites — the app's connect, the form's Test button, `cli::run::connect` — so no
caller has to remember it: while it was the caller's job, two of the three had none.
**`open_tunnel` refuses a SQL Server named instance before any I/O** (`tunnel_target_refusal`,
its first line): the instance's port is SQL Server Browser's answer over UDP, the tunnel
forwards TCP alone, and dialled as a name `server\INSTANCE` resolves to nothing — so the
question cannot be asked through it. The refusal says to set the bare host and the port the
instance listens on, and sits in `open_tunnel` rather than its callers for the bounds' reason:
every caller gets it (`a_named_instance_is_refused_through_a_tunnel`, which asks `open_tunnel`
itself). It asks `connection::sql_server_instance` alone, not the engine, so any host spelled
that way is refused — on another engine such a host names no server either. **Its three callers
pass `Connection::tcp_target()`**, not the raw fields, so a SQL Server host's `tcp:`, `(local)`
and inline `,port` are resolved before the tunnel dials — `db\SQLEXPRESS,1500` is tunnelled to
`db:1500` rather than refused (S5-L1-02). `tcp_target` hands a named instance's host over *as
saved*, and that is load-bearing: the bare server in its place would drop the `\` this refusal
reads, and the tunnel would dial whatever listens on the saved port instead.
**Jump hosts are hops `open_tunnel` walks before the SSH host** (`SshTunnel::route`, under
`connection.rs`), as `ssh -J` does: the first is `client::connect` over TCP, each later one
`client::connect_stream` over a `channel_open_direct_tcpip` of the previous session's
`into_stream()`, so the database's host need be reachable only from the last. Each hop is a
tunnel of its own in every respect that matters for trust — its own `TunnelClient`, so the TOFU
check runs against *its* `host:port`; its own `within` bounds, so the 20 s and 60 s above apply
per hop and not to the route as a whole; and `authenticate` as the hop's user, which is why
`authenticate` and `authenticate_key` take a `user` now. An unreadable route is refused before
any I/O with `route()`'s sentence. **The jump sessions are moved into the accept loop's task**
and held there unused: each carries the transport of every hop after it, and dropping one —
the obvious tidy of a binding nothing reads — cuts the tunnel. On a multi-hop route a failure
names the host it happened at (*SSH connect to h:p failed*, *SSH jump host could not reach
h:p*), where a direct one keeps *SSH connect failed*; the info line logs the whole route.
Verified live by the change's author, not by a test — the tier does not cover SSH (see below):
a throwaway sshd in WSL with key auth, routes of 0, 1 and 2 jump hosts through it to the
MySQL 8.4 container all returned the server greeting, and a jump to a closed port was named in
the error.
**The SSH host and each jump host may be a `~/.ssh/config` alias** (`core::ssh_config`, under its
entry). `core::ssh_config::read_user_config` reads `<USERPROFILE or HOME>/.ssh/config` once per
`open_tunnel` call,
as `ssh` reads it per connection — so an edit applies at the next tunnel opened, while one already
open keeps the route it was opened by, since nothing watches the file — refusing a file over
`MAX_CONFIG_BYTES` and decoding it with `text::decode_text_file`. A missing, unreadable or
oversized file is no error: the tunnel opens on the connection's fields as written. `open_tunnel`
takes its route from `core::ssh_config::resolve_route`, which resolves the tunnel through
`resolve` + `apply` **before** `route()`, so a `ProxyJump` the file
supplies is parsed by the same reader as a typed one — and one it cannot read is refused with the
same *Jump host N: …*, about a string the form never showed — then each jump hop, all but the
last, through `apply_to_hop`; the last is the SSH host, already resolved. It also refuses,
before any I/O, a route the file hides in a `ProxyCommand` or a `Match` block for the SSH host
or its first jump host (whose own `ProxyJump` refuses too), and a password bound for a jump host
only the file names — all under that entry, and the form's hint says each of them first
(`describe_route`). The `RouteRefusal` reaches the connect's error as `SshFailure::Route`, and
the window words it as the form does. Known-hosts entries
are therefore keyed by the **resolved** `host:port`, not the alias. The connection form keeps the
alias in its field and says what it became on a line under it (`ssh_config_hint`, under
`connection_form.rs`) — from its own read of the file, taken when the tunnel's fields are built,
so after an edit to the file the form can lag the connect until it is reopened; the info line's
route and an error's `h:p` name the resolution too. Verified live by the change's author, not by a test: a
throwaway root sshd in WSL on `127.0.0.1:2222` and a temporary home whose config had `Host
zzalias` (`HostName`, `Port`, `User`, `IdentityFile ~/client`, `ProxyJump zzjump`) and `Host
zzjump` (`HostName`, `Port`); a tunnel naming only host `zzalias`, key-pair sign-in, with no
user, key or jump of its own, reached the MySQL 8.4 container and its greeting came back, and the
known-hosts entry written was `127.0.0.1:2222`. The two refusals and the comment reading were
checked the same way, with a password-only user added to that sshd: an alias routed by a
`ProxyCommand`, one routed by a `Match originalhost` block, and password sign-in through a
config-only `ProxyJump` were each refused with their sentence **and no known-hosts entry
written**, so nothing was dialled; an alias whose `Host`, `HostName`, `Port` and `User` lines
carried trailing comments, a key-pair route through a config `ProxyJump`, and password sign-in
through the same jump host typed into the connection all returned the greeting.
**PostgreSQL cannot connect without naming a database**, which is protocol rather than
preference and stayed invisible while almost every server had a `postgres` one anybody could
reach — so `connect_maintenance` guessed at that, the username and `template1` for server-level
work. A hosted provider need not publish any of them: on Aiven only `defaultdb` is permitted,
every guess is refused by `pg_hba`, and the server was unreachable with nothing in the form to
correct it (Test connection, the database list and the schema tree all route through that probe).
`Connection::database` is the fix, read through `default_database` so SQLite — whose file *is*
its database — answers `None` however the engine picker left the field. It is the probe's
**first** candidate rather than its only one, since a database since dropped should degrade to
the old guesses instead of locking the user out of the server; `maintenance_candidates` holds
that order, deduplicated and without an empty username, as a tested decision rather than a
literal inside an I/O loop. On MySQL the same field is the database a connection opens in when
none is selected, and a *fallback* rather than an override — an operation that named one is
working in it. MySQL stays strict where Postgres cannot be: a database that does not exist fails
with `Unknown database 'x'`, which beats silently opening nowhere, and Postgres has no such
option because it must connect somewhere. The probe also **stops early**: only a database-level
failure (`3D000`, or `28000`, which reads as an auth error but is matched per database) is worth
another candidate, because repeating a rejected certificate or a wrong password once per
candidate is how a misconfigured certificate came to report "timed out" rather than naming
itself. A failed init statement is not one either: it would fail the same way in every
candidate, so `connect_probe` returns it with the retry flag cleared.
**A connection's init SQL runs inside each engine's one connect function, right after it
connects and before the caller sees the connection** — what may be in it is `core::init_sql`'s.
`Db` carries it as `init_sql`, filled by both arms of `Db::connect` and empty from `from_parts`,
which the MCP endpoint handoff fills back through `with_init_sql` from what `init_sql()` wrote
(`schemaic-app`'s `ai.rs` entry). Every connect asks **`Db::init_statements`** before any of it
runs — `init_sql::check` for this engine, its refusal a `DbError::Refused` reading *Not
connected — this connection's init SQL was refused: …* — so a refused init runs none of it; the
three networked engines ask before they dial. A statement the server then refuses is
`Db::init_failed`, a `DbError::ConnectFailed` naming it (`ConnectFailure::InitFailed`, *init
SQL `…` failed: …*), and the operation
fails with it. **That is deliberate**: a `SET ROLE` that did not take would run the operation as
the login's own role, so nothing goes on in a session missing a setting the user asked for, and
the fix is the connection form. The four sites: **MySQL's is `Db::dial`**, which every `open`
and `open_serverless` passes through, calling `mysql::run_init` — in `mysql.rs` rather than
inline because `the_dispatcher_executes_nothing_itself` forbids `lib.rs` a `query_drop`;
**PostgreSQL's is `connect_probe`**, by `batch_execute`; **SQLite's is `sqlite::open`**, by
`execute_batch` after its `busy_timeout`, and a pragma a caller sets after the open —
`foreign_keys` on the write paths — wins over the init's; and **SQL Server's is
`mssql::connect`**, now the check, then `login` (the old body, up to the login), then each
statement through `simple_query` — **not `execute`**, which sends an RPC, and a `SET` inside
`sp_executesql` lasts only as long as that call, so the setting would be gone before the
operation began. After a non-empty init it sends **`SET IMPLICIT_TRANSACTIONS OFF`**, belt and
braces behind `init_sql::check`: under implicit transactions every auto-commit write is rolled
back at the disconnect while the app reports it done, and `ANSI_DEFAULTS ON` once turned them on
from an init the old deny list let through. Whatever the init set, the operation starts in
auto-commit; a refusal of that statement is `Db::init_failed` like any other
(`mssql::an_init_sql_never_leaves_implicit_transactions_on`, which also has `ANSI_DEFAULTS ON`
refused before the connect). **Running before the caller sees the connection is what keeps it below the
read-only guard**: every `Enforce` statement is the caller's, issued afterwards (under
`fetch_query_enforced`, above). The one connection it skips is `pg::cancel_query`'s, a
`CancelToken` that runs no SQL; the terminal's database client is opened by none of these and
does not run it either, and nor do *Back up*'s and *Restore from file*'s tools — bar a PostgreSQL
init SQL's `SET ROLE`, which `pg_dump`, `pg_restore` and `psql` are handed as a role
(`core::launch::init_role`); MySQL's tools and `sqlcmd` get none of it, no equivalent option
being wired. Without a server, `sqlite.rs`'s
`the_init_sql_runs_on_every_connection_an_operation_opens` and
`a_refused_init_sql_refuses_the_connect_before_any_of_it_runs` pin both halves; the server legs
are the live tier's `init_sql.rs` and `mssql::the_init_sql_is_in_force_and_a_refused_one_fails_the_operation`,
below.
