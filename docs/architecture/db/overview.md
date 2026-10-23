# `schemaic-db`

`schemaic-db` — MySQL/MariaDB (`mysql_async`) in `mysql.rs`, PostgreSQL in `pg.rs`,
SQLite in `sqlite.rs`, SQL Server (`tiberius`, vendored) in `mssql.rs`, SSH tunnels in
`ssh.rs`, and
the pinned manual-transaction connection in `session.rs`.
**Three engines and three modules — the move is finished.** For most of the crate's life MySQL
had no module: its bodies were inline in `lib.rs`, so `pg.rs` and `sqlite.rs` were peers of each
other and of nothing else, and the crate doc opened by warning about it. Six steps moved one
family of entry points each — account introspection; Server Activity and table statistics;
`collect_schema` with everything it folds plus the four lazy `SHOW CREATE` reads; the wire
decoders and the read paths; DDL, scripts, imports and write-back; and finally `run_script` with
the convention flip — taking `lib.rs` from 9,590 lines to about 3,100 beside `mysql.rs`'s 7,055, and
leaving no `Engine::MySql` arm in `lib.rs` that is not a call into `mysql::`. (The one dispatch
arm in the file that still builds SQL is SQLite's inside `fetch_table`, which is not one of
`ENGINE_ENTRY_POINTS` and so is outside the census below.) Both convention tests read
three engines now, and the second is renamed for it:
`every_engine_module_answers_the_whole_interface` and
`the_dispatcher_calls_every_engine_module_for_every_entry_point`. **`mssql.rs` is a fourth module
and answers every name**, the unwritten ones with a refusal: it is in the first test's list and
in `the_entry_point_list_is_what_the_dispatcher_actually_dispatches`'s intersection, whose
listed-⊆-called half is what holds its dispatch arms, and the second test's own loop names all
four modules.
**Flipping them to three is what found the last three doors.** `commit_writes`, `refetch_rows`
and `fetch_blob` existed in `mysql.rs` only as `write_on`, `refetch_on` and `blob_on` — the
bodies `session.rs` calls directly — while the dispatcher's arm was still inline, so the interface
had three names MySQL answered only from the inside. Both tests named all three, which is the
census doing what the compiler cannot: a fourth engine *variant* is a compile error at every
dispatch site, a module answering all but one of the names is not.
**But both of those measure the modules and the dispatcher *against* the list, so a name that
never reached the list is a name neither of them asks about.** One never had: `run_batch` is
`pub(crate) async fn` in all three engine modules and dispatched to all three, and was written
down nowhere — both gates were green over thirteen of fourteen, and putting SQLite's `run_batch`
back on the `fetch_query` loop that once cascade-emptied child tables passes them.
`the_entry_point_list_is_what_the_dispatcher_actually_dispatches` derives the set the other way:
every name the dispatcher calls on **all three** modules is an entry point whatever the list says,
asserted in both directions, with a floor so that an empty intersection cannot satisfy the first
half having read nothing. `run_batch` is on the list, which was fourteen then and has since gained
`fetch_server_vars` and `fetch_table_stats` — the second once SQLite's sizes came from `dbstat`,
its SQLite arm having answered an empty set inline in the dispatcher until then; the const's doc
carries no count at all now, having said "ten of thirteen" and "twelve of thirteen" about a list
that was already fourteen long.
**And the gate that most needed comments stripped was the one not stripping them.**
`the_dispatcher_calls_every_engine_module_for_every_entry_point` scanned `include_str!("lib.rs")`
whole while its siblings filtered `//` lines first, and a rustdoc link with parens reads exactly
like a call — so a doc comment naming `sqlite::run_script` could stand in for the dispatch arm
somebody deleted, in the file that is largely doc comments about which statement lives where. It
and `the_entry_point_list_is_what_the_dispatcher_actually_dispatches` share a `dispatcher_code()`
helper now.
**The census also caught a mistake byte-identity could not.** One step moved a *line range*
rather than an item list and swept three public `impl Db` methods — `commit_writes`,
`refetch_rows`, `fetch_blob` — into `mysql.rs` with it, where they compiled perfectly, an
inherent `impl Db` block being legal in any module of the crate. The moved lines were
byte-for-byte what left; it was *which* lines that was wrong, and the thing that said so was
`the_dispatcher_calls_every_engine_module_for_every_entry_point`: "nothing in the dispatcher calls
`pg::fetch_blob`".
**Two of this crate's source gates had to be repaired for the move, each in the way a source gate
fails — by passing.** `only_one_function_decides_which_mysql_family_server_this_is` opened
`src/lib.rs` by name and asserted no line re-derives `contains("mariadb")`; once `collect_schema`
moved, that scan would have found nothing, because there was no MySQL introspection left in
`lib.rs` to offend, while a second spelling in `mysql.rs` went unread. It walks every `.rs` in the
crate's `src` now, with a floor (`scanned >= 5`) so a moved directory fails rather than scanning
nothing, and it was watched red by planting an offender in `mysql.rs`. The second failure is the
mirror of it: **a gate that ends up in the same file as its subject becomes its own first
match**, so a literal `find("async fn import_on(")` finds the test's own line before the function
and the "body" it then measures is twenty lines of the test. The import gate's floor caught that
one — *only 0 notes* — which is what a floor is for, and the timeout gate's first version failed
on `ping` for the same reason. Both needles are assembled from fragments now, the way the
`mariadb` gate always did it.
**What stays in `lib.rs` is decided by who reads a thing, not by what it is named**:
`assemble_schema`, `ColRow`, `IdxRow`, `FkColRow`, `TxScope`, `DdlError`, `lock_wait_sql`,
`next_batch_off_executor`, `order_by_clause` and the
`NumKind`/`num_kind`/`parse_as`/`parse_typed` family are all called from `pg.rs` despite their
MySQL-flavoured vocabulary, and moving any of them into an engine module would make one engine
depend on a module named for another; `ident_sqlite` is the mirror-image trap, sitting
next to MySQL's own `ident` and belonging to neither. The rule covers the tests as well, which is
the half that was got wrong: the `assemble_schema_*` tests travelled to `mysql.rs` with one step
and had to come back, a test for a function two engines share not belonging in one engine's
module. **The other direction is the one the rule is easiest to lose in**: `err_clone` — which
rebuilt a `DbError` for the "connect failed" fan-out, `DbError` not being `Clone` then — stayed
in `lib.rs` when its only caller moved to `mysql.rs`, and was *widened* to `pub(crate)` so the
call would still reach it. Widening to survive an extraction is exactly the shape the rule
refuses: it was read by one engine, so it went to live beside that engine, private again, which
the compiler then proved was the whole of its audience. (It is gone now: `DbError` became
`Clone` for `DdlCause::Db`.)
**`mysql::run_batch` takes two arguments `pg::run_batch` does not** — the `USE` scope and the
dialect — because `USE` is MySQL's alone, so only that arm can move the batch's current database
mid-run and only it has to make a later statement's result label follow. That is the concrete
answer to why the engine interface is a **naming convention** (`ENGINE_ENTRY_POINTS`) rather than
a trait: a convention carries an asymmetry a trait would have to flatten into a parameter every
engine takes and two ignore.
**The last statement `lib.rs` sent itself is gone, and that is a property now rather than a
claim.** `Db::kill_query` — a `query_drop` of `KILL QUERY <id>` — is
`mysql::kill_query(db, conn_id)`, which is where every one of its callers already was: thirteen in
`mysql.rs`, and four in `session.rs` inside `Backend::MySql` arms. Those four have since become
calls to `mysql::cancel_awaited`, which kills *and awaits*, so `session.rs` no longer names
`kill_query` at all — and a gate of its own now requires that (see the cancel rule below).
`the_dispatcher_executes_nothing_itself` scans `lib.rs` for the driver's own verbs, with `//`
lines stripped first, and fails on any of them. **Thirteen verbs in two shapes, and the list's own
length is pinned.** A suffixed verb (`query_drop`, `query_iter`, `query_first`, `query_map` and
the four `exec_` spellings, plus `query_internal`) can only be the driver's, so the name alone is
the needle; a short one (`query`, `exec`, `prep`, `batch`) is a word this file uses in other
senses all day — `fetch_query`, `run_batch`, `ENGINE_ENTRY_POINTS` — so it is matched with the
method dot that makes it a call rather than a name. `_map` is the pair the gate was written
without and the pair the extraction actually carried out of the file: `Db::fetch_grants` and
`Db::fetch_table_list` were `query_map` at `v0.25.0` and both still are, in `mysql.rs`, so putting
either back here left the gate green. Nothing can make this gate fail without putting a call back,
so `assert_eq!(verbs.len(), 13)` is the only thing that says a verb has been dropped from the
list — which is exactly how `_map` went missing. It is named for the driver verbs rather than for
SQL because a
scan for keywords cannot tell a statement from a doc comment about one, and this file is largely
doc comments about which statement lives where; the verbs are **assembled** from two arrays for
the reason the timeout census next door assembles its `stop` marker, the first spelling of the
test having tripped on its own source. Read the rule narrowly: it is not "no statement text is
left in `lib.rs`". `lock_wait_sql` and `TxScope`'s `BEGIN`/`COMMIT`/`ROLLBACK TO SAVEPOINT` stay,
because more than one engine reads them and they are strings an engine module *runs*. What has
gone is `lib.rs` executing one.
**The payoff beyond the rule is that the two spellings of `KILL QUERY` are adjacent — and still
deliberately two functions.** `mysql::kill_session` had been writing `KILL QUERY {id}` for
`KillKind::Query` all along, a file away. It answers the Server Activity panel: gated on a
capability, takes the `i64` the server reported, returns an error the panel shows, and opens
`db.open(None, ..)`. `mysql::kill_query` is the *cancel* path — reached because a statement this
app is itself waiting on has to stop, taking the `u32` the driver reported for its own connection,
ignoring failure because the caller's `select!` has given up on the statement either way, and
opening `Db::open_serverless`, since a `KILL` names no object and a connection that first has to
open the user's database is one more thing that can hang on a server already misbehaving. Merging
them would have to flatten all four of those differences; sitting next to each other is what lets
the next reader see they are the same sentence to the server.
**One remainder is named in the crate doc rather than left to be discovered**: `Db`'s connection
plumbing is still MySQL's. `open`, `open_serverless`, `opts`, `opts_with_tls` and `dial` all speak
`mysql_async` and nothing in `pg.rs` or `sqlite.rs` calls any of them — those two build their own
clients. It sits there because it belongs to the handle rather than to an operation, and moving a
type's constructor out of the module that defines the type is a different question from moving its
bodies. That is the answer rather than an open question; `TODO.md` used to carry it as one, and no
longer does. **`Db::fetch_sessions`/
`Db::kill_session`** are the Server Activity panel's whole backend, and they are up to three
queries per engine rather than one: MySQL runs `information_schema.PROCESSLIST` (required —
without it there
is no panel) plus `INNODB_TRX` and a lock-wait join, both *best effort*, because those two are
what need `PROCESS` privileges and what differ by server. The lock-wait join has no single
spelling — MySQL 8 removed `information_schema.INNODB_LOCK_WAITS` and MariaDB has no
`performance_schema.data_lock_waits` — so the pair is tried in turn, and when neither works the
panel still knows *who* is blocked (`trx_state = 'LOCK WAIT'`) and simply cannot say by whom,
which is the honest degradation.
**On a quiet poll — nearly every poll — the join is not run at all.** With a fixed order exactly
one engine always pays a guaranteed failure, and since MySQL 8's spelling goes first that engine
is MariaDB: measured on 10.11, ERROR 1146 on every tick, 1,800 an hour at the two-second interval,
forever, to learn nothing. `collect_sessions` asks `activity::wait_graph_is_worth_fetching` over
the `INNODB_TRX` states it has already read — a thread announces itself as a waiter there — so the
question is answered from a table in hand and the round-trip goes away on *both* engines. It is
asked rather than remembered because a `Db` is built fresh by `Db::connect` per operation, so a
cache of which spelling worked would be discarded between polls; a poll that does find a wait
still pays MariaDB's failure, which is the rare case the panel exists for.
PostgreSQL needs one query: `pg_stat_activity` filtered to
`backend_type = 'client backend'` (the checkpointer and the WAL writer are processes, not
sessions, and would sort to the top forever) with `pg_blocking_pids(pid)` for the graph, which
resolves the transitive case a hand-written `pg_locks` join gets wrong. A row whose `pid` won't
parse is **dropped**, never admitted under id `0`: a session `0` renders a full row with a live
"Kill session" under it and can be pointed at by other rows' edges, which is the same reasoning
`parse_pid_array` already applies to the graph. **Both exclude the caller's own connection**, as
SQL Server's query does now (`@@SPID`, under `mssql.rs`) — every operation here opens a fresh
one, so the poller would otherwise report itself running the activity query at the top of every
refresh. **Both also sort blocked-or-working sessions above
idle ones before the `LIMIT`**, and that is not cosmetic: `activity::rank` puts lock waits at the
top of the panel, but a session that started waiting four seconds ago has the *smallest* age on
the server, so ordering the fetch by age alone — which reads like "keep the interesting end" —
cut every row of a lock pile-up on a box holding three thousand idle pool connections and left a
quiet-looking list during an incident. PostgreSQL sorts exactly (`blockers <> '{}'`, read out of a
subquery so `pg_blocking_pids` is evaluated once per backend rather than again for the `ORDER BY`);
MySQL settles for `COMMAND <> 'Sleep'`, because `PROCESSLIST` carries no lock information and the
view that does needs the `PROCESS` privilege this required statement deliberately doesn't. Both
methods gate on `activity::supports_activity` / `supports_kill` **before** the engine `match`: the
`match` picks a query set, which no capability can paper over, but the predicate is what stops a
fourth engine falling through the MySQL arm into `information_schema.PROCESSLIST` and failing
three catalogue lookups deep. Kills are `KILL QUERY` /
`KILL CONNECTION` and `pg_cancel_backend` / `pg_terminate_backend`, always on a new connection,
since the session being killed may be the one holding everything else up. **`Session::server_id`
exists for this panel**: MySQL's thread id comes back in the handshake and PostgreSQL's backend
pid is asked for at open, and holding it is what lets the app recognise one of its own pinned
Manual-mode connections in a list of session ids — see the repair below. SQLite errors rather
than answering empty (`core::activity::supports_activity`); the app is gated not to ask.
**Both are bounded by a deadline around the whole engine dispatch** — `fetch_sessions` by
`PING_TIMEOUT`, `kill_session` by `CANCEL_TIMEOUT` — and neither was. A host that stops answering
at the packet level does not refuse the connect, it swallows it, so the `open` takes the OS TCP
timeout: 21.0 s on MySQL, 63 s on PostgreSQL, this crate's own measurement. `fetch_sessions` is
the method that most needed it, running **on a timer, forever**:
a poll against a black-holed host blocked for 21 s showing the previous snapshot with no error,
beside a health check that had already said *Disconnected* at five seconds — and the polls
*stacked*, because regaining window focus re-runs the panel effect, which bumps the activity
generation and then refreshes, so the in-flight guard is keyed on a generation the refresh has
just moved and cannot suppress it. Three alt-tabs inside one connect left three connects hanging
at once. The deadline is the fix that does not depend on that guard. `kill_session` earns
`CANCEL_TIMEOUT` for the reason `mysql::kill_query` already cites — the premise of reaching it is
that something on that server is misbehaving — and its timeout is reported as an error rather
than as a claim the kill failed: the statement may well have landed, and the next poll settles it.
Around the dispatch and not inside each arm, for `Db::fetch_databases`' reason: PostgreSQL's
`connect_maintenance` tries three candidate databases in turn, so a per-attempt bound is three
times the deadline it claims. `every_reachability_path_is_bounded_by_a_timeout` is a source gate
over `ping`/`fetch_databases`/`fetch_sessions`/`fetch_locks`/`fetch_server_vars`/`kill_session`
**and `fetch_table`**, because the failure is a host that never answers and no unit test can stage
one — a closed port is *refused*, instantly, and only a packet filter reproduces the hang. **Its table names a file per method,
because the bound is not always where the public method is.** Two shapes exist and both are
right: `fetch_sessions`, `fetch_locks`, `fetch_server_vars`, `kill_session` and `fetch_table`
wrap the whole dispatch
in `lib.rs`, so the bound belongs to the public method, while `ping` and `fetch_databases` are
bounded per engine — `pg::ping` takes the deadline as a parameter, and SQLite's wraps an `open` that can block on a
dead network share. Writing the file down per method is what stops the gate reading a file its
subject has left, which is how a source gate stops testing anything without failing.
**`fetch_table` is the app's second forever-timer, and this gate's own doc used to say in so many
words that `fetch_sessions` was the only one.** The Live
Monitor re-arms `fetch_table` every two seconds for as long as its modal is open, and the premise
that admitted it to the already-bounded set — *the unbounded reads all take a
`CancellationToken`* — was true of the signature and false of the caller, which built a token
inline, stored it nowhere and cancelled it never. A dark host therefore cost the OS connect
timeout on every tick, under a modal still showing the last snapshot with no error, beside a
health check that had said *Disconnected* at five seconds. Its deadline is `PING_TIMEOUT`, and the
trade is worth naming: a poll that cannot finish inside it is one the monitor's two-second cadence
could not have kept up with anyway, and saying so beats a silent stale snapshot. The list the gate
reads is *the set of methods a timer calls*, so a method joining that set belongs in it.
`fetch_locks` is the newest to join: the Locks view polls on the sessions list's timer, so it
takes the same `PING_TIMEOUT` around the same dispatch. `fetch_server_vars` is on the list
**without** a timer — the Server variables modal reads once per opening, per switch and per
Refresh — and that is a widening of the rule above, stated rather than slipped in: it is admitted,
as `kill_session` is, for being the call made when somebody is looking at a server, which is
when a dark one is likeliest, and a modal sitting on *Loading…* for the OS connect timeout is
the failure the deadline exists to bound.
**`Db::fetch_locks` is Server Activity's *Locks* view**, gated on `supports_activity` like
`fetch_sessions` and one query per engine. Each engine asks for `MAX_LOCKS + 1` rows ordered
waits first, so the server's own cut keeps the waits, and each leaves out its own session; the
dispatch then runs `activity::prepare_locks` and ORs its verdict into the engine's `truncated`.
PostgreSQL (`PG_LOCKS_SQL`) reads `pg_locks` LEFT JOIN `pg_database`, with an ungranted row's
blockers from `pg_blocking_pids` — the function the sessions list already trusts over a
hand-written join. **It connects to the connection's own database where `db.database()` names
one**, and to the maintenance database only otherwise, unlike the sessions list: `pg_locks` is
cluster-wide, but a relation's oid is a name only inside its own database, so `::regclass` can say
`public.orders` there and nowhere else. A relation in another database is written
`db: relation <oid>` and **never cast**, since the same oid here may be some other table.
`transactionid`, `virtualxid` and `advisory` locks get labels of their own; a *granted*
`virtualxid` — every transaction's lock on itself — is dropped, while a waited-for one, the
`CREATE INDEX CONCURRENTLY` kind of wait, stays. MySQL and MariaDB keep their locks in tables
neither has of the other's — MySQL 8 removed `INNODB_LOCKS`, MariaDB never had
`performance_schema.data_locks` — so `mysql::fetch_locks` **asks `SELECT @@version` once and
decides through `activity::mysql_locks_in_information_schema`** (which asks
`ServerFlavour::parse_version`) rather than trying one spelling and
falling back, which is the shape that cost MariaDB a failed query on every sessions poll (above)
until `wait_graph_is_worth_fetching` learned to skip it. Its first draft decided the flavour with
a hand-rolled `contains("mariadb")`, and `only_one_function_decides_which_mysql_family_server_this_is`
caught it; it then decided by flavour alone, and **MySQL 5.7** — which has `INNODB_LOCKS` and no
`data_locks` — answered every poll with "Table 'performance_schema.data_locks' doesn't exist".
MySQL 8 (`MY_LOCKS_PS_SQL`) reads `data_locks` and `data_lock_waits` through
`performance_schema.threads`, because both name a *performance-schema* thread rather than the
processlist id a session is known by everywhere else; the object is
`schema.table (INDEX) LOCK_DATA`. MariaDB and MySQL 5.7 (`MY_LOCKS_IS_SQL`) read `INNODB_LOCKS` joined to
`INNODB_TRX`: a lock is waiting when it is its transaction's `trx_requested_lock_id`, its blockers
come through `INNODB_LOCK_WAITS`, and `lock_table` arrives backticked and is shown without them.
**Both answer `contended_only = true`**, because `INNODB_LOCKS` lists a lock only while it is
waited for or holds up a wait — a quiet list there is not a lock-free server, and the panel says
so. **Every engine's query ends in `LockInfo::killable`, by its sessions query's own filter**:
MySQL 8 a `FOREGROUND` thread whose user is neither NULL nor `system user` and whose command is
not `Daemon` (on 8.4 `compress_gtid_table` has processlist id 7 and user NULL, and
`event_scheduler` is a `Daemon`); MariaDB and 5.7 the same test on the thread's
`information_schema.PROCESSLIST` row, LEFT JOINed; PostgreSQL `pg_stat_activity`'s
`backend_type = 'client backend'` or the masked-row branch, LEFT JOINed on the pid, so a
parallel or autovacuum worker — and a pid gone since — is not offered a kill; SQL Server
`sys.dm_exec_sessions.is_user_process`. SQL Server (`mssql::locks_sql`) reads `sys.dm_tran_locks`. An `OBJECT` lock is named across
databases, `OBJECT_SCHEMA_NAME`/`OBJECT_NAME` taking the database id; a `KEY`, `PAGE`, `RID` or
`HOBT` lock names a heap or index that `sys.partitions` resolves only in the database this
connection is in, so elsewhere it shows the database and the engine's own `resource_description`.
**Each named concatenation sits under a `COALESCE` whose fallback is the `db: object <id>`
label**: `OBJECT_NAME` is NULL for an object the login has no metadata access to, or one dropped
since, and `+` carried the NULL through the whole cell, leaving the lock unnamed and unsearchable
(`a_lock_object_the_server_will_not_name_keeps_its_database_and_id`).
Blockers come from `sys.dm_os_waiting_tasks` on `lock_owner_address`, and besides `@@SPID` it
leaves out every session's granted shared `DATABASE` lock, which is held by existing and would be
most of the list. **The names are asked for under `SET LOCK_TIMEOUT 1000`, and dropped when they
would block**: `OBJECT_NAME` waits behind another session's `Sch-M` under any isolation level
(Msg 1222 with READ UNCOMMITTED, measured on 2022), so a lock list asked for while an `ALTER
TABLE` was stuck hung to the poll's deadline — exactly when it was wanted. On 1222 the query is
re-run with `locks_sql(_, false)`, which labels objects `db: object <id>` and skips
`sys.partitions` (`the_lock_list_answers_while_ddl_holds_a_table`). SQL Server also answers a
row per lock *owner*, so `prepare_locks` merges identical rows; one `ALTER TABLE` came back
twenty times. Under the full live tier's load the two SQL Server lock tests can still meet the
poll's deadline, and pass alone — a load flake, not a wrong answer.
What the live tests saw (below): MariaDB `db.imp (PRIMARY) 1`, `RECORD` `X`;
MySQL `RECORD` `X,REC_NOT_GAP` beside a `TABLE` `IX`; PostgreSQL the waiter on `transaction N`,
`transactionid` `ShareLock`, with relation and tuple locks on `imp` and `imp_pkey`. The same
test asserts both sessions' locks are `killable` on every leg, and passed against a MySQL 5.7.44
container through the `INNODB_LOCKS` arm.
**`Db::fetch_server_vars(kind)` is the Server variables viewer's backend**, beside the two above
and on their terms: it asks `server_vars::supports_server_vars` and, for `VarKind::Status`,
`supports_status` **before** the engine `match`, and a status list asked of an engine without one
is a `DbError::Query` saying so rather than an empty list, which would read as "no counters".
One read per engine, all reshaped to `core::server_vars::ServerVar`. MySQL/MariaDB
(`mysql::fetch_server_vars`) run `SHOW GLOBAL VARIABLES` or `SHOW GLOBAL STATUS` — **`GLOBAL`,
not the session's**, because the session is this operation's fresh connection: its values are
the server's defaults plus whatever the driver set on connect, which is not what anyone opening a
server's settings is asking. The rows are read as bytes and decoded leniently, so one value that
is not UTF-8 cannot lose the rest of the list; there is no description to show. PostgreSQL
(`pg::fetch_server_vars`) reads `pg_settings` with the unit appended to the value and
`short_desc` as the detail, on `connect_maintenance` for the sessions list's reason — these are
the cluster's settings, not one database's. SQL Server (`mssql::fetch_server_vars`) reads
`sys.configurations` — the `sp_configure` options — taking `value_in_use` rather than a `value`
still awaiting `RECONFIGURE`, and casting it and `description` to `nvarchar` so the read never
depends on how the driver decodes a `sql_variant`. SQLite (`sqlite::fetch_server_vars`) asks
`sqlite_version()` and then each of `server_vars::SQLITE_PRAGMAS` on one `with_conn`
connection; a PRAGMA this build of SQLite does not know answers no row and is left out rather
than shown blank. The pins are `sqlite::tests::server_vars_lists_the_version_and_the_pragmas`
(in-memory, shared-cache) and two live tests: `suite::server_variables_list_the_servers_settings`
in `live_suite!`'s `suite` group (`max_connections` on every leg, `Uptime` in the status list
where `supports_status`, an error elsewhere) and
`mssql::server_variables_read_the_servers_configuration` (`max degree of parallelism` with a
description; the status list an error).
**`Db::fetch_principals`/`Db::fetch_grants`** are the Users and privileges browser's backend, and
they gate on `users::supports_users` before the engine `match` for the same reason those two do,
with `NO_USERS_MSG` as the one sentence both raise. The MySQL half is `collect_my_users`: **five
queries, of which exactly one runs to completion** — MariaDB's `mysql.user` column set, then MySQL
8's, then `MY_USERS_ROLE_SQL`, then the bare `(User, Host)` pair, then
`information_schema.USER_PRIVILEGES`. **The fallbacks
fire on an *error*, not on an empty result**, the rule the lock-wait pair above had to learn:
`mysql.user` is never legitimately empty, so an empty answer would mean the read was denied and
only the error says so. The middle rung asks for **`is_role` alone**, for a MariaDB with roles but
no `password_expired` — every 10.1, 10.2 and 10.3, a window still in service. Without it such a
server fell through to the pair below, where `is_role` is `None` on every row, so
`from_mysql_rows` folded each role into a `User` and kept the `''` host MariaDB stores: a role
`readers` listed as `readers@`, and three of the four actions built from it are the errors
`core::users` already documents (1141, 1133, and a `DROP ROLE` grammar with no `@host` in it).
Role-ness is the one column whose absence changes what a statement *is*, which is why it gets a
rung rather than sharing the pair's. The last query is the one whose failure the caller sees, and it is the one
an *application* account can actually read — `mysql.user` needs `SELECT` on the `mysql` database,
which a properly-provisioned account has not got, while `USER_PRIVILEGES` shows it its own row: one
account is a poor list, but it is a true one and it is the account the person opening this is most
likely asking about — **and reaching it is news, so that rung is the one that sets
`Principals::note`** (`users::PrincipalsNote::MyOwnAccountOnly`). The three `if let Ok` rungs above it
discard their errors deliberately, a denied read being the expected case, which is exactly why the
absence has to be carried out rather than inferred from a count of one.
**The two widest rungs no longer project the same shape, and that is the compiler holding them
apart rather than a comment asking.** `my_user_rows(rows, mariadb: bool)` slotted the fifth column
into `is_role` or `account_locked` by a pair of boolean literals twelve lines apart, and
transposing them makes every locked MySQL account a `Role`, drops its host, and leaves
`DROP USER "app"` resolving to a different account. It is `my_mariadb_rows` (five columns) and
`my_mysql_rows` (six) now, with a `MyUserTupleMysql` beside `MyUserTuple` rather than a sixth
`Option` on one shared tuple, because MySQL's rung asks for a column MariaDB's does not:
`IF(LENGTH(authentication_string) > 0, 'Y', 'N')`, the **presence** of a stored credential and
never the hash, compared on the server so nothing credential-equivalent crosses into the process.
It is the third of the three flags MySQL's own `CREATE ROLE` leaves and it is what withholds
**Reset password** from a row the catalogue cannot tell from a role — the fingerprint
`core::users` documents — while MariaDB's rung does not need it, `is_role` answering there
outright, and the `None` it leaves means *not published* rather than *no credential*. Three tests
hold that seam: `the_fifth_column_lands_in_the_field_this_servers_spelling_meant`,
`the_credential_flag_reaches_the_fold_only_on_the_rung_that_asks_for_it` — which runs the mapping
through `from_mysql_rows` to the `role_ambiguous` it decides, a dropped flag being invisible to
`core`'s own tests — and `the_credential_column_is_asked_as_a_presence_not_a_value`, which counts
the occurrences of `authentication_string` in the query so that "just select the column and
compare here" cannot be the next edit. `my_role_rows` is the third rung's three; everything else
is `users::from_mysql_rows`. `fetch_grants` runs
`SHOW GRANTS FOR <account_sql>` and pipes **every row through `users::redact_secrets` at the
boundary**, not in the view, so a second caller — the grant/revoke step beside it, a copy
button — cannot forget to. `pg::fetch_principals` reads `pg_roles` on the **maintenance**
connection (the catalogue is cluster-wide and the browser may be open with no database selected)
and, unlike `pg::roles` behind the Owner dropdown, **keeps** the `pg_` predefined roles, since they
hold real privileges and `users::from_pg_rows` sorts them last. **It reads the server's
`users::PasswordPolicy` on the same connection** (`pg::password_policy`), so a
`password_encryption` set per role is the one the plan's role gets:
`current_setting('password_encryption')`, `current_setting('scram_iterations', true)` — the
`true` because the setting only exists from 16 — and the `pg_settings` row for
`shared_preload_libraries`, which is simply *absent* for a role without `pg_read_all_settings`
rather than an error, so the names of any `passwordcheck.%`/`credcheck.%` settings are asked
too, credcheck's being listed to everyone once it is loaded. A failed read is `None`, the SCRAM
default, and never fails the list. What it does not see is a setting made per *database* on the
one the plan will run in, this being the maintenance connection. The MySQL half passes `None`.
`pg::fetch_grants` is **five
queries, because PostgreSQL has no `SHOW GRANTS`**: `pg_auth_members`, then `aclexplode` over
`pg_database.datacl`, `pg_namespace.nspacl` and `pg_class.relacl` (relkinds `r p v m f S`, filtered
rather than left open so an index or a composite type can't reach the statement builder, and `S`
the only one taking the `SEQUENCE` keyword). Three of those see **one database only**, which is
what `users::GrantsNote::PgScope` says on screen — and the two per-database ones are **not run at all**
with no database selected, since the client then stands on PostgreSQL's *maintenance* database and
would answer for a database the user did not choose; its `database: None` form says that instead. The
three ACL queries `ORDER BY … is_grantable, privilege_type`, which is what makes
`pg_grant_statements`' one-pass grouping — comparing each row against the group before it — see
each `(object, grantable)` group whole. The fifth query is not an ACL read at all: it asks for
`rolsuper` and a count of the objects this role owns (`pg_database.datdba` +
`pg_namespace.nspowner` + `pg_class.relowner`), which feeds `users::pg_implicit_note` and rides
into the scope note as its `implicit`. Every one of them compares against the role's own oid,
so a role dropped since the list was fetched yields an empty answer rather than an error. `pg_bool`
is the small reader `simple_query`'s text protocol needs, which spells a boolean `t`/`f`.
**`Db::fetch_principals` takes a `database` because SQL Server's accounts are half one
database's**: the logins are the server's and the users one database's, so the app passes the
browser's database, and MySQL and PostgreSQL ignore it, their accounts being server-wide.
`mssql::fetch_principals` and `mssql::fetch_grants` are the SQL Server half, under `mssql.rs`.
Populates each result column's
`origin` (real table/column + key flags) from the wire protocol. Connection **identity** is the
`Db` handle (`Db::connect(&Connection, tunnel_port)`), not a `mysql://…` URL — credentials go
through `OptsBuilder` (passwords with `@ / # ? %` need no escaping; no plaintext URL anywhere).
Schema introspection fills the **full** column model (see `core::schema`): MySQL from
`information_schema.COLUMNS` (`COLUMN_DEFAULT`/`EXTRA`/`COLLATION_NAME`/`COLUMN_COMMENT`/
`GENERATION_EXPRESSION`) + `STATISTICS` (`SUB_PART`/`COLLATION` for prefix + `DESC`, and
`schema::index_disabled_sql`'s interpolated `CASE` for an index the DBA has switched off — the
same hold-the-row-shape trick the view `ALGORITHM` below plays, and what makes `IdxRow::lossy`
read `expression || disabled` rather than the functional key part alone). **A functional key part
is the one row where `STATISTICS.COLUMN_NAME` comes back NULL**, which is why that bind is
`Option<String>` with `EXPRESSION` selected beside it — the non-`Option` `String` it used to be
panicked `mysql_async`'s `from_row` *inside the spawned fetch task*, before any `Result` existed
to map, so one such index anywhere left the whole database unbrowsable: the schema tree spun on
"loading" for ever, with no error row and nothing to retry against. It was fixed collaterally by
a commit that cited only the two findings it set out to close, so it stood fixed and untested;
`mysql::a_functional_index_does_not_stop_the_schema_being_read`
asserts on `fetch_schema` itself rather than on the fold, because a unit test of the fold is green
against the unfixed tree. MariaDB 10.11 rejects `((a + b))` outright and PostgreSQL keeps
`pg_get_indexdef`'s whole text so is correctly not lossy — two of the three legs could not
reproduce it, which is the same reason two servers hid it originally
(`Target::expression_index_sql`). **An `IdxRow` is one column of one index, keyed or
included**: `IdxRow::included` sends the row's column name to `IndexInfo::include` rather than
to the key, and `assemble_schema`'s fold finds or creates the index first and then pushes onto
whichever list the row belongs to, in arrival order — the readers sort key rows first. Folded
into the key, a covering `(a) INCLUDE (b)` would read as `(a, b)`, a different index that a
rebuild or an edit would then create (`assemble_schema_folds_included_columns_apart_from_the_key`).
SQL Server and PostgreSQL set the flag; MySQL and SQLite never do. PostgreSQL
from **`pg_catalog`, not `information_schema`** — `format_type(atttypid, atttypmod)` is the only
source of the *declared* type (`udt_name` gives `varchar`, losing the `(45)`), plus
`pg_get_expr` for defaults and `attidentity`/`attgenerated`. **That leaves two PostgreSQL
type-name spellings, and the grid's is the other one.** `pg_type_name_str` maps the wire
`Type` and was described as "the single type-name mapping" back when the schema side was fed
`udt_name` from it; the move to `format_type` made `pg_type_name` its only non-test caller, so the
grid header says `VARCHAR` where the tree and the completion popup say `character varying(45)`.
That display divergence is open. What the two must agree on is the **reading**: both spellings
have to reach `schema::classify_column_type`, `ddl::normalize_type` and `model::type_is_binary`
as one type, or one surface's icon, diff or binary-cell guard answers differently from the
other's for the same column — which is what `the_two_type_spellings_are_read_as_one_type` pins,
and what the test it replaced claimed to hold while comparing the function with itself.
`mysql_column` normalizes the
MySQL/MariaDB `COLUMN_DEFAULT` divergence (MariaDB returns SQL text, MySQL a raw value needing
quoting) — pure + tested, since getting it wrong writes a *different* default rather than failing.
MySQL additionally reads each table's engine/collation/comment, each FK's
`REFERENTIAL_CONSTRAINTS` rules, and each view's `CHECK_OPTION`/`DEFINER`/`SECURITY_TYPE`
(`mysql_view_options`; `ALGORITHM` too, but only on MariaDB — MySQL 8 has the column
nowhere but `SHOW CREATE VIEW`, so the query holds its shape with a `CAST(NULL AS CHAR)`
and `Db::view_algorithm` reads it **lazily, per view**, when the editor opens: one
`SHOW CREATE VIEW` each is too many round-trips for a schema fetch, and the value is only
needed for the view being redefined. `view_algorithm_of` is the pure reader — it takes the
clause *positionally*, between `CREATE` and `DEFINER`, since everything past that is the
user's own SQL and can say `ALGORITHM=` too);
CHECK constraints come from `CHECK_CONSTRAINTS`, which the two servers shape differently
(MariaDB has `TABLE_NAME`, MySQL 8 needs a `TABLE_CONSTRAINTS` join that is also the only
home of `ENFORCED`) — and only `ER_UNKNOWN_TABLE`/`ER_NO_SUCH_TABLE` degrades to "no checks",
for servers predating the feature, so a broken query surfaces instead of silently emptying
every table's constraints. **`mysql_check_clause` is the third of these
MySQL-vs-MariaDB text divergences** (with `mysql_column`'s defaults and the view
`ALGORITHM`), and the rule is again "normalize on the way in": MySQL 8 returns
`CHECK_CLAUSE` with one *extra* level of backslash escaping — `_latin1'new'` arrives
as `_latin1\'new\'` — where MariaDB returns it byte-for-byte runnable. Restating
MySQL's verbatim is a **syntax error**, and unescaping MariaDB's would eat the
backslash out of `'it\'s'`, so it is gated on the `mariadb` flag and measured against
`SHOW CREATE TABLE`. It also has to run *before* `ddl::check_predicate`, whose paren
scan reads string boundaries that `\'new\'` doesn't have.
**`CHECK_CLAUSE` is not the only column carrying that extra level**, which is why the unescaper
has a second caller: `COLUMN_DEFAULT` carries it too for an *expression* default, the arm `EXTRA`
flags `DEFAULT_GENERATED`. `DEFAULT (CONCAT('a','c'))` comes back as
`concat(_utf8mb3\'a\',_utf8mb3\'c\')` (HEX-verified), and restating that is `ERROR 1064` rather
than a subtly different default — so that arm of `mysql_column` routes through
`mysql_check_clause`, which short-circuits on MariaDB for the same reason it always did. The
character-set introducer survives the unescaping, and where the whole value is one literal
(`_utf8mb3'draft'`, which is how MySQL 8 reports a string it recorded as an expression default)
`schema::is_bare_default` reads it as the literal it is rather than wrapping it for having a
prefix;
**The standalone PG objects** come from `pg_types` (enums *and* domains in one `pg_type`
scan — both live there, `typtype` `e`/`d`) and `pg_sequences`, each folded by a pure,
tested half (`pg_fold_types`/`pg_sequence_row`). Four decisions are written down because
each is a bug the live pass would otherwise have shipped: an enum's labels arrive **one
row each** rather than string-aggregated, since a label is arbitrary text and every
separator is a value some database already stores (`'a,b'`, an embedded newline, `''`);
a domain's "has a default" is its own column, because `DEFAULT ''` read off the text
would come back as *no* default and get dropped on every replay; a domain's collation is
reported only when it differs from its base type's, or every `text` domain would open
with a phantom `COLLATE`; and a sequence's definition is read from the `pg_sequence`
**catalogue** while only `last_value` comes from the `pg_sequences` **view**, so a role
that may see the schema but not the data gets a full definition and a blank position
rather than a vanished sequence. These run inside `fetch_schema`, where one failure takes
the whole database's schema with it, so they go through `query_all_optional` — which
degrades on `undefined_table`/`column`/`function` only, the same two-sided rule
`mysql_checks` follows.
PG reads `confdeltype`/`confupdtype`, the `pg_constraint` name behind a PK/unique index,
and its view bodies from **`pg_get_viewdef` over `pg_class`, not `information_schema.views`**
(which hands back an empty definition to a non-owner and omits materialized views entirely)
plus `reloptions` for the storage params a replace would reset (`pg_view_options`) — all
folded on *after* the shared `assemble_schema` (which both engines share and neither's
extras belong in). **That fold looks each table up rather than filtering for it**: the checks,
triggers and the per-namespace row sets are bucketed once by `group_by` and the FK rules keyed
by `(namespace, table, constraint)`, where a `.filter` per table (and per namespace) and a scan
of every FK rule per table made the load O(tables × rows) — measured 1.25 s against
0.24 s on a 2400-table database with one CHECK, trigger and FK each. `group_by` lives in `lib.rs`
beside `assemble_schema`, because both engines read it: `pg_fold_types` buckets an enum's labels
and a domain's constraints by `(namespace, type)` through it rather than filtering and cloning
both row sets per type, MySQL's `apply_check_constraints` and `apply_triggers` bucket by table
name, and `apply_fk_rules` — which does not call it — builds a name → table-index map once, the
first table of a name winning as the per-rule `find` did. **The checks and triggers are folded by
a pure function**, `pg_fold_checks_and_triggers`, defined after `collect_schema` in the shape of
`pg_fold_types` and MySQL's `apply_check_constraints`: it takes the three row sets (checks,
triggers, `UPDATE OF` columns) as the owned `Vec<Vec<Option<String>>>` `query_all` already
returns, so nothing about the wire stands between it and a unit test. It used to sit inline in
the per-table loop with no test at all, while the MySQL folds beside it had theirs. It builds the
`UPDATE OF` map itself, keyed `(namespace, table, trigger)` — two tables may each have a `trg` —
and `a_check_goes_to_the_table_of_its_own_schema` and
`a_trigger_takes_its_own_update_of_columns_in_order` pin the two keys.
**`table_list_sql`'s fourth column is the table comment.** It is the one
table option PostgreSQL has and nothing was reading it: the designer emits `COMMENT ON TABLE`
correctly and this query never selected `obj_description`, so a comment set through the app was
**write-only** — it vanished from the app the moment it landed, and re-applying could not restore
it either, a draft whose comment matches the empty read raising no change at all. Found by
writing the live tier's `a_table_comment_lands_and_reads_back`, which failed on exactly that
before the four-line read was added.
**Its third column types `relkind = 'f'` as `FOREIGN TABLE`.** The query already listed foreign
tables and typed them `BASE TABLE`, so nothing downstream could tell one from a table and every
table action was offered on it, `DROP TABLE` included, which the server refuses on one.
`assemble_schema` (under `lib.rs`) and `pg::fetch_table_list` both set `TableInfo::is_foreign`
off that value, so the first-paint list and the full read agree (`a_foreign_table_is_typed_as_one`;
live, `a_foreign_table_is_read_and_dropped_as_one` asserts both reads). What follows from the
shape is under `schema.rs`'s `TableShape`.
**Its last four columns are the partitioning**: a partition's parent, namespace and name, through
`pg_inherits` (joined only where `c.relispartition`, so a table that merely inherits is not taken
for one); its bound, `pg_get_expr(relpartbound)` — `FOR VALUES …` or `DEFAULT`; and a
partitioned table's key, `pg_get_partkeydef` — each empty where it does not apply. The pure
`pg_fold_partitions` puts them on `TableInfo::partitioning` and `partition_of` (under
`schema.rs`), and runs in `fetch_table_list` as well as the full read, so the first-paint list
already knows which table is a partition of which (`the_object_list_reads_the_partitioning`,
`partitions_fold_onto_their_parent_and_name_it`). A parent may sit in another namespace than its
partitions, which is why the parent's namespace is a column and the fold keys on the pair.
**What PostgreSQL clones onto a partition is read and marked, never restated**: `index_list_sql`
selects `ic.relispartition AS inherited` — an index attached to its parent's — and the
foreign-key query `con.conparentid <> 0`, a key with a parent constraint, into
`IndexInfo::inherited` and `ForeignKeyInfo::inherited`. They stay in the model because a
partition's grid edits key on its primary key, which is one of them; they are kept out of every
script because the partition gets each again from its parent when it is created, and a second
`CREATE INDEX` or `ADD CONSTRAINT` of the name fails the restore. A cloned trigger is the one
clone not in the model at all — `tgparentid = 0` filters it out (below).
**`pg_extensions` fills `DbSchema::extensions`** from `pg_extension` joined to `pg_namespace` for
`extnamespace`, and is **deliberately not on `user_schema_filter`**: an extension is the
database's rather than a namespace's, and `plpgsql` lives in `pg_catalog`, which that filter
hides. `pg::available_extensions(db, database)` reads `pg_available_extensions` on a connection
to that database — the view's `installed_version` is the connected database's — behind
`Db::available_extensions(database)`, which answers empty on the other three engines and has
`Db::roles`' standing: it feeds a shortcut beside a free-text field, so a failure costs the
picker's chevron and never the name.
**`index_list_sql` reports one row per key position, and its `lossy` term is what the model
admits it did not read.** It was a non-default opclass and a non-default `indoption`; it is now
also `ic.reloptions IS NOT NULL` (a storage parameter such as `fillfactor`, which nothing had
asked for) and `ix.indnatts > ix.indnkeyatts AND pgc.conname IS NOT NULL` — an `INCLUDE` list on
a primary-key or unique **constraint**'s index, which stays lossy because neither restatement
carries one: the model holds a key as a column list and writes a unique constraint as
`ADD CONSTRAINT … UNIQUE`. **A plain index's `INCLUDE` list is read**, into
`IndexInfo::include`. `indkey` is `indnatts` long and carries the list past the key, but
`indoption` is only `indnkeyatts` long, so the `indoption` unnest is a `LEFT JOIN` — as an inner
join it dropped every `INCLUDE` position before `pg_attribute` was consulted, and the
`indnatts > indnkeyatts` term, then unconditional, was the admission — `descending` is
`COALESCE`d to false for the positions it no longer reaches, and a trailing
`k.ord > ix.indnkeyatts AS included` marks each row the fold sends to the list (under `lib.rs`).
The live `a_partly_read_index_says_so_and_is_emitted_whole` reads `ix_inc (a, b) INCLUDE (c)` as
key `[a, b]`, list `[c]` and not lossy, its dump round trip still passing, while `ix_nd` and
`ix_ff` stay lossy. **`NULLS NOT DISTINCT`
cannot be asked of the catalogue at all**, and that is the load-bearing part:
`pg_index.indnullsnotdistinct` is PostgreSQL **15**, the server parses the whole statement before
it runs it, and naming a column a 13 or 14 has not fails the *entire* introspection — both are
still in support. So `pg_indexdef_is_lossy` asks the question of `pg_get_indexdef`'s **text**
instead, which every supported server renders. It matches uppercase, because that is how the
server writes a keyword and a case-insensitive match would let an index *named* `nulls not
distinct` answer yes; the clause can still be spelled inside a quoted identifier, and that costs an
edit withheld rather than an index destroyed — the direction `IndexInfo::lossy` is written to fail
in. The same `pg_get_indexdef` column fills `IdxRow::create_sql`, so PostgreSQL now carries the
server's own statement per index the way SQLite always has, which is what `TableInfo::create_ddl`
replays for a lossy one. Both new terms are string-tested on the query
(`lossy_no_longer_covers_what_the_model_can_hold`, which pins the `LEFT JOIN` and the
`included` column too), and
`a_nulls_not_distinct_index_is_read_as_lossy_from_its_own_ddl` asserts the fourth is answered from
the text **and** that the PG 15 column is still absent from the query.
**`col_meta_sql` is the other primary-key reader in this file, and the two disagreed.** It is
`fetch_col_meta`'s query, extracted so the decisions in it can be checked without a server the way
`table_stats_sql`, `index_stats_sql` and `index_list_sql` already are — three judgments live in
that string (which columns are the key, which column is server-assigned, and the ordinals the fold
reads by position) and each has a wrong answer that produces a plausible grid rather than an
error. The key was the wrong one: `PRIMARY KEY … INCLUDE` has been legal since PostgreSQL 11 and
`pg_index.indkey` is `indnatts` long, so an unbounded `unnest(i.indkey)` counted an INCLUDE column
into the primary key; the subquery is bounded by `indnkeyatts` now. `index_list_sql` got the same
question right only **by accident**, inner-joining `unnest(ix.indoption)`, which is only
`indnkeyatts` long — and that join is a `LEFT JOIN` now, to read a plain index's `INCLUDE` list,
so `pk_set` excludes an `included` row by name rather than leaning on it, since the primary
index's included rows now arrive with an `attname` the old filter would have counted into the
key. The two readers disagreeing meant the answer depended on whether the schema tree had
introspected the table yet, an
everyday difference for a table created since the last refresh, where `edit::resolve_key`'s
no-schema branch trusts these wire flags. With `PRIMARY KEY (id) INCLUDE (payload)` the key came
out `[id, payload]`: a binary `payload` made the whole grid silently read-only, and a non-binary
one put a non-identity value in every `UPDATE`'s `WHERE`, so any concurrent touch of it made the
statement affect no rows and the batch roll back reporting the row as gone. The live case is
`an_include_column_is_not_part_of_the_write_key`, with `Target::primary_key_include` carrying the
clause for the one leg that has it.
**One read reports one result set, and both engines now agree on that.** A PostgreSQL simple query
is one string and the server will happily run every statement in it — the MCP server hands one
straight through and nothing upstream splits it — so `pg::run_statement` counts the
`RowDescription` messages, the only boundary marker on that stream, and stops at the second. It
used to fall through the `_ => {}` arm that `SimpleQueryMessage`'s `#[non_exhaustive]` requires, so
the second set's rows were pushed through the *first* set's columns and cell kinds: reproduced live
through `Db::fetch_query`, a two-statement string came back as 3 rows under one column header, the
third being 1,282 characters of hex from a `bytea` read under set one's `INT4` — the case `pg_cell`
exists to prevent. That is the answer MySQL's `collect_rows` has always given by construction, and
breaking out is safe for the same reason the row cap's `break` is: the driver's connection task
keeps paging, so the connection stays reusable, which a Manual-mode tab depends on. The
non-exhaustive arm stays, and the question to ask of anything new arriving there is whether it
carries a boundary or a column list, because those are what the loop's state rests on. The
column-metadata `PREPARE` describes `sql::first_statement(sql, PG)` rather than the whole string
for the same reason: `Parse` takes a single command, so a multi-statement string cannot be prepared
at all, and a failed prepare leaves every `type_name` empty — an empty type name is an unknown
`CellKind`, which is how the `bytea` came to be read as text in the first place. For a single
statement it is the same string and the same one round trip, terminator included.
**Which columns a set is read under is `result_columns`**, split out of `run_statement` so that
decision has a seam: the prepared columns where the describe gave any — so a zero-row `SELECT`
still reports its columns and every cell is parsed by its real type — else the first row's names
alone (a `RowDescription` over the simple query protocol carries no type this crate reads), else
`None`, which is a DML/DDL/utility statement reporting affected rows and no grid. The three
coupled pieces of state behind it decide whether a `bytea` renders as `<n bytes>` or dumps its
whole hex encoding into a cell, and none of them had a test on any tier. **The middle arm is
lossy and knowingly so**: `type_name` is empty there, so `type_is_binary("")` is false and a
`bytea` is read as text. It is reached whenever the *first* statement of a multi-statement string
returns no rows — `SET`, `BEGIN`, `ANALYZE`, a `DELETE` without `RETURNING` — since
`first_statement` describes that one and it prepares to zero columns. Closing it means describing
the first row-returning statement, or re-describing by set index here; either way this is where
the answer changes, and `the_columns_a_result_is_read_under` is the test that would flip.
**No error here is built from the driver's own `Display`; a driver error goes through `db_err`.**
`tokio_postgres::Error`'s `Display` is a *category* — pinned at 0.7.18 it renders every
`ErrorResponse` as the literal `db error` — so the six **read** paths that spelled
`DbError::Query(e.to_string())` put `query failed: db error` in the schema tree while the server
had said `canceling statement due to statement timeout`. `db_err` was already here and already
used at the eight *write* sites, and its own doc already said why, so the defect was purely which
of two compiling spellings each site chose — which is what makes the pin a scan of this module's
own source (`no_error_in_this_module_is_built_from_the_drivers_own_display`). It prefers
`as_db_error`, and its non-server arm walks `error_chain`, since a failed TLS handshake keeps its
cause in `source()`.
`pg_message(message, detail, hint)` is the other half: the diagnostic is three fields and the
second is the one that names the *value* (`Key (pid)=(9) is not present in table "par".`, and on
a multi-column key or a bulk `INSERT … SELECT` it is the whole of it), `HINT` is where *Perhaps
you meant to reference the column …* lives, `psql` prints all three, and `DbError` carries a
`String`, so anything dropped here cannot be recovered downstream. It is a function of its own
only because a `tokio_postgres::Error` cannot be constructed by hand and this is the seam a test
can reach — the same reason `error_chain`'s tests use a synthetic chain. Live:
`a_refused_write_says_which_value_the_server_refused`, with `Target::error_names_the_value`
recording which engines carry a separate detail field at all.
**`DbError::modal_source` answers whose words an error is** (`core::model::ErrorSource`), read off
the variant, for the editor's error bar and the error modal behind it: `Query` is a statement's
failure, with SQL for *AI fix* to rewrite — and so is `QueryFailed`, below; `Connect` — and
`ConnectFailed`, below — is the
server's words about no statement, explained and never fixed; `Cancelled` is the user's Stop or Schemaic's own watchdog and has
nothing to explain; `Refused` is **Schemaic's own sentence** and has nothing to explain either.
`Refused` exists because the variant is the provenance and this crate had been writing its own
words as `Query`/`Connect`, so the bar and the modal offered *AI fix* or *Explain* on them. It
carries a `core::model::DbRefusal` (re-exported as `schemaic_db::DbRefusal`), not a `String`,
and every producer names its variant: `Db::init_statements` (`InitSql`), `sign_in_secret` and
`mssql::config` (`Transport`, from `AuthMode::transport_refusal`), `backup_to_file`'s server
arm, `session::aborted_commit`, which `Session::commit` answers over a block that was already
aborted, `Session::open`'s SQLite manual-mode backstop (a `Connect` once) and
`Session::open_enforced`'s refusal of a read-only SQL Server connection (`MsSqlReadOnlyAuto`,
under `mssql.rs`),
`mysql::enforce_session`'s refusal of a client character set that can hide a quote and
`unpinnable_mode` — its refusal when the server puts a quote-moving `sql_mode` back — and
`read_only_plan_refusal`, SQL Server's Windows and cloud IAM sign-in arms, `explain_setup`'s
read-only gate (`read_only_refusal(..).reason`), `kill_session`'s cancel, the blank-string and
code-page checks, `sqlite::open_target`'s three refusals, SQLite's "the SQLite worker failed: …"
(a `Query` once) at each of its six sites, its default-reset refusals and `table_stats`' size
cap. It renders **as written**, `#[error("{0}")]` with no prefix — `DbRefusal`'s `Display`, its
English: each is already the whole sentence, and "query failed:" in front of "Commit cancelled
— …" named a failure that was the user's own act. The window words it instead
(`i18n::db_refusal_text`, from `db_error_shown`); the CLI, the MCP server and the logs keep
`to_string()`. SQL Server's `err_text` no longer names `Refused` in its pass-through, its
`Display` being the same text.
**`Write(WriteFailure)` is a write path's failure carrying what its rollback achieved**
(`core::model::WriteFailure`), built by the crate-private `DbError::write(cause, undone)` at
every exit that used to append `Rollback::note` to a `Query` or `Refused` string: MySQL's
`write_on` and `import_on`, PostgreSQL's and SQLite's one-row verdicts and import batch counts,
SQL Server's `failed`/`failed_as` and `WriteScope::failure`, and the non-`Complete` arms of
`cancelled_import`/`cancelled_write` on MySQL and SQL Server ("Import/Commit cancelled — …",
MySQL's two having been `Refused` producers until then). It displays as `WriteFailure::message`, **byte for byte the
`Query` or `Refused` it replaced**, so the log and every test reading `to_string()` saw no
change — the CLI and the MCP server reach none of these paths — and what changed is that the
app can word it (`db_error_shown`, under `app/main.rs`).
Its `modal_source` follows the same predicate as the English prefix —
`WriteCause::is_statements`, `Statement` where the message says "query failed:", `App` over a
Stop or a refusal — so it lands where the variant it replaced did. **Only the exits that carry
a rollback became `Write`** — and one producer that never does: `row_failure`, a row source's
`WriteCause::Row` built with `undone: None` for the engine to complete (under `Db::import_rows`
below), which reaches the interface as a `Write` even where PostgreSQL and SQLite return it
bare. A server error PostgreSQL's write paths return bare, through
`db_err`, is still a `Query`, as is one under SQL Server's savepoint, which carries no note
(`WriteScope::failure`, under `mssql.rs`), and both reach the window as any `Query` does, its
"query failed:" worded by `db_error_shown` and the server's words inside left as they came.
`modal_source_tests`' `only_a_statements_failure_is_offered_a_fix` holds five `Write` cases to
that predicate — a server error and a batch count `Statement`, the two cancels and a refusal
`App`; the SQL Server import's live tests match the variant
(`an_imported_blank_number_is_refused_not_stored_as_zero`).
**`ConnectFailed(ConnectFailure)` is a connect failure in Schemaic's words around someone
else's** (`core::model::ConnectFailure`, re-exported with `SshFailure`, `SshPhase` and
`TlsFailure`). It shares `Connect`'s `#[error("connection failed: {0}")]`, its `modal_source`
(`Server`) and its `position` (`None`), so moving a site from one to the other changed nothing a
reader of `to_string()` or the modal could see; what changed is that the app can word it
(`db_error_shown`, under `app/main.rs`). Its producers are every sentence this crate composed
around another's text at connect: every "timed out" site (`ConnectFailure::TimedOut`, fourteen
spellings across `lib.rs` and the four engines' modules), `Db::init_failed`,
`session::retired_error` and `begin_failed`, `enforce_session`'s prepare,
`pg::read_only_setup_failed` and `connect_maintenance_avoiding`'s *no maintenance database
reachable*, SQL Server's session guard and plan set-up, its two SQL Server Browser sentences and
its two for a waking Azure SQL database (`BrowserSilent`, `BrowserWithoutTcp`, `AzureTimedOut`,
`AzureUnavailable`, which were `Connect(String)`s of English until the review after v0.35.0
found them), every `ssh.rs` and `tls.rs` failure
but one (their passages, below), and every failure `cloud_iam.rs` and `entra.rs` meet asking a
CLI for a token, through each one's private `sign_in_failed`. **`Connect(String)` stays for a
driver's or the system's text alone** — SQLite's `{file}: {e}`, the tunnel listener's
`local_addr` — where there is no frame of Schemaic's to word. Keeping the driver's text there is
load-bearing: `unknown_database`, which decides whether an
unopenable configured database is recoverable, reads only a `Connect`'s message, so the
driver's own connect error moved to `ConnectFailed` would stop being recognised. SQL Server's
`err_text` passes `ConnectFailed`'s `Display`, without the prefix, as it passes a `Connect`'s
string.
**`QueryFailed(QueryFailure)` is the same for the `Query` sentences this crate writes itself**
(`core::model::QueryFailure`, re-exported as `schemaic_db::QueryFailure`). It shares `Query`'s
`#[error("query failed: {0}")]`, its `modal_source` (`Statement`) and its `position` (`None`),
so again nothing a reader of `to_string()` or the modal could see changed. Its producers are
`writer_gone` (`ExportStopped`), `stream_query`'s no-rows refusal, `fetch_server_vars`' two
refusals, `import_rows`' no-columns refusal, `copy_rows`' read that stopped, SQL Server's
`batch_error` (`BatchContinued`, under `mssql.rs`), SQLite `query_err`'s lock arm
(`SqliteLocked`) and the SQLite import's rollback that failed too (both under `sqlite.rs`); SQL
Server's `err_text` passes its `Display` without the prefix. **Four sentences stay a plain
`Query` on purpose**: `COUNT(*) returned no row`, *the count came back as … not a number* and
*A copy needs the multi-threaded runtime* are internal invariants failing, and MySQL's
`Unknown database '{db}'` mimics the server's own text.
**The server refusing a statement Schemaic sent for itself is a connect failure, not `Query`** — its words, but about SQL the user did not write, and theirs
was never sent: `enforce_session`'s "could not prepare the session: …" (`PrepareSession`),
`pg::read_only_setup_failed` ("could not make the session read-only: …", `ReadOnlySetup`, for
`pg::fetch_query`'s enforced path and `Session::open`'s PostgreSQL arm) and
`session::begin_failed`, which rewrites a lazy `BEGIN`'s `Query` from `Session::control_on` as
"could not begin the transaction: …" (`BeginFailed`) and passes anything else through — each a
`ConnectFailed`. The `Connect` prefix does come with them, so these read
"connection failed: could not …". **`modal_about_another_statement`** is
`ModalError::new(to_string(), modal_source()).about_another_statement()`, for an error raised by
something that is not the editor's statement (a grid Commit, `end_tx`); it is one composition
because the commit arm had restated it by hand and got `Refused` wrong. The grid Commit, `end_tx`
and a failed `Session::open_enforced` now ask the app's `modal_error_shown`, its twin *shown as*
`db_error_shown` words it (`ModalError::shown_as`), so the error reaches the modal in the
interface's language with the same source — while its `text` stays `to_string()`, the English
*Explain* sends the model.
`app/main.rs`'s run paths ask it of the `DbError` a run ends in rather than
choosing a source per site, which is how a refused connection stopped being offered a fix into
the buffer. Those paths answer `Cancelled` in arms of their own first — the not-sent, timeout and
cancel notes — so its `App` there is the answer for a caller that has not. **Named
`modal_source`, not `source`**: `std::error::Error` already has a `source()`, and an inherent
method shadowing it would answer a different question under the same name. One test per variant,
from `a_rejected_statement_is_the_statements_failure`; `mod modal_source_tests` holds all four
together, the off-editor composition and `a_refusal_renders_as_written`, and `session.rs` pins
its two producers (`a_failed_begin_is_not_the_statements_failure`,
`an_aborted_commit_is_schemaics_report`).
`fetch_query`/`fetch_query_enforced`/`stream_query`/`stream_query_enforced`/`run_batch`/
`run_batch_enforced`/`fetch_schema`/`ping`/
`commit_writes`/`refetch_rows`/`prepare_check`
(non-executing `PREPARE` for the editor's live validation)/`run_ddl`/`run_script`/`fetch_table_stats`/
`count_rows`/`fetch_blob` are `Db` methods taking the target DB per call.
`backup_to_file(target)` takes a *file* instead, and answers on SQLite alone
(`sqlite::backup_to`); the three server engines get `DbError::Refused`, inline in the dispatcher,
their backups being their own tools run in the terminal (`core::launch`).
**`Db::fetch_blob` is the binary-cell panel's whole backend** — one connection, one `SELECT`, one
row, like everything else here. `Ok(None)` means the cell is SQL `NULL` **or** the row is gone,
and both are "there are no bytes to show" rather than an error the panel would have to phrase.
The statement is built per backend beside `build_update` and *not* in `core::blob`, for the same
reason that one is not: MySQL and SQLite bind the WHERE key as parameters where PostgreSQL
inlines it. `build_blob_select` is MySQL's — `SELECT OCTET_LENGTH(c), SUBSTRING(c, 1,
LEAST(?, PACKET_ROOM)) … WHERE k <=> ? LIMIT 1`, the NULL-safe `<=>` included, because the
identity of a row is one thing here whether it is being written or read, and `SUBSTRING` on a
*binary* string is byte-indexed so the caps really are octets. **Two caps, and the smaller
wins**: ours (`FETCH_CAP`, bound as a parameter so the statement carries no second literal) and
the server's (`PACKET_ROOM` — `GREATEST(1024, CAST(@@max_allowed_packet AS SIGNED) - 1048576)`,
read live *inside* the same statement). Neither subsumes the other, and the server's is not
theoretical: a row has to fit in one `max_allowed_packet`, and on MySQL the blob really does cross
the wire where PostgreSQL's and SQLite's do not. MariaDB ships that setting at 16 MiB, a
**quarter** of `FETCH_CAP`, and asking for the full 64 MiB did not fail politely — measured
against MariaDB 10.11, a 20 MiB `LONGBLOB` dropped the connection mid-row (`ERROR 2013` at the
client), which on a manual-transaction tab's pinned session takes the uncommitted work with it.
Capped, the same read returns 15 MiB of bytes beside the true `OCTET_LENGTH` of 20 MiB, so
`BlobValue::truncated()` answers `true` and the panel shows the prefix and refuses to save it —
which is the behaviour the truncation design already had. The setting is read inside the statement
rather than in a round trip of its own, so there is no window for it to change between the asking
and the reading and no second query on a path that is one click; the 1 MiB of headroom is for the
rest of the packet and is deliberately generous, because being 100 bytes over costs the whole
connection while being 1 MiB under costs a truncation the panel already knows how to describe.
`pg::blob_on` is `octet_length` + `substring … from 1 for N`, and it is **the one read in that
module on the extended protocol** (`client.query`, not `simple_query`) — two decisions at once,
both verified live against PostgreSQL 16.15. The bytes come back **binary**: `bytea`'s textual
form is `\x` and then two characters per byte, so at the 64 MiB cap the text path built a ~128 MiB
`String` and decoded it into the `Vec` after, a quarter of a gigabyte in flight for one cell. And
a `Parse` takes **one** statement where `simple_query` speaks the protocol that accepts several:
this is the only grid path reachable from a single click with no write verdict on a connection the
user marked read-only, so it was the widest reach the key-in-statement-text shape had —
`client.query("SELECT 1; DROP TABLE …")` is refused and the table survives, where `simple_query`
runs both. The key is **still** built into the statement text, because binding it generically
would break every key type outside `Value`'s four variants: `col IS NOT DISTINCT FROM $1` infers
`uuid`, `date` and `numeric` from the column (confirmed on PG 16.15) and such keys are
`Value::Str` today, reaching the server as literals it coerces. So the pinned
`standard_conforming_strings` is still what closes the escape, and the extended protocol is a
second, structural line behind it. `bytea_hex_bytes` is **gone** with its only caller;
`bytea_hex_len` stays, because the *result-set* path is the text protocol by design, and its test
was rewritten to pin the counter alone rather than against the departed decoder.
`sqlite::fetch_blob` is `length` + `substr`, both byte-wise only *because* the **value** really
is a blob — the per-value half of `blob_source`'s two signals, not the column's, since a SQLite
`BLOB` is an affinity and a text row in such a column would be counted in characters here.
**Reading in binary also retired the one place a fetch could fail on the *encoding* rather than
the row.** A server with `bytea_output = escape` sent a form `bytea_hex_bytes` refused, raised as
an error naming the setting rather than substituted with an empty buffer — pairing no bytes with a
real `octet_length` would report 35.5 KB over an empty dump and call it truncated. There is no
text encoding left on this path to be wrong about, so that error is gone rather than merely
unlikely. **The length and
the bytes come out of one row of one statement**, never two queries: asked separately they can
straddle another session's `UPDATE`, and the pair is what `BlobValue::truncated` reads to decide
whether a save would write a file that is not the data. `Session::fetch_blob` (MySQL,
PostgreSQL and SQL Server) runs the same body on the pinned connection for `Session::refetch_rows`' reason: a
manual-transaction tab that has written a blob and not committed would otherwise open the panel
on the pre-transaction bytes and call them current. Like every other `Session` call site, the app
folds its `Outcome::stmt` into `TxState::on_statement`: a read is still a statement, and one that
loses the connection has to reach the tab or the footer pill goes on claiming a live transaction
over a dead socket.
**Both reads on the pinned connection are fenced by a savepoint, and that is not symmetry for its
own sake.** `Session::fence_read` puts a `SAVEPOINT` in front of `fetch_blob` and `refetch_rows`
and `Session::classify_fenced` takes it back out — released on success, rolled back *and* released
on failure or cancellation, upgrading the outcome to `FailedIsolated`/`CancelledIsolated` only if
the server accepted the rollback (the same "confirmed, not assumed" rule `classify_isolated`
follows; `ROLLBACK TO SAVEPOINT` does not release, and a transaction can hold a great many blob
reads). On PostgreSQL a cancelled statement aborts the enclosing transaction exactly as a failed
one does, and every way of dismissing the binary-cell panel cancels — Escape, the ✕, the footer,
clicking a second binary cell — so closing it mid-read discarded every uncommitted statement of a
Manual-mode tab, on the least consequential thing that connection does; `refetch_rows` runs
immediately after a `commit_writes` the user has just watched succeed, so what it would discard is
work they were told they had. `commit_writes` has had a savepoint and `classify_isolated` since it
was written and the reads had neither. Reproduced live on PostgreSQL 16.15. A server that refuses
the `SAVEPOINT` gets the old, honest classification rather than a claim the fence cannot back, and
`fetch_blob` also gained `fetch_query`'s pre-dispatch `cancel.is_cancelled()` check on **both**
sides of the connection lock: the overwhelmingly common cancellation there is the panel being
dismissed, and `NotSent` is the one outcome that leaves an *open* transaction alone with no
bookkeeping at all. **SQL Server's reads are not fenced, and need not be**: `run_scope_sql`
answers `false` there without asking, because a T-SQL savepoint cannot be released, and a
cancelled read does not abort a SQL Server transaction — see `mssql.rs`.
**Both reads also leave through `Outcome::into_read`, and that is the half the fence cannot
cover.** `fence_read` has no savepoint to set when there is no transaction to fence — PostgreSQL
answers `ERROR: SAVEPOINT can only be used in transaction blocks`, so `run_scope_sql` reports
`false` and the read runs unfenced, while MySQL accepts a `SAVEPOINT` outside a transaction
(measured on MariaDB 10.11.14) and so hid the whole class. Neither of these two paths calls
`ensure_tx`, so neither may report a transaction it did not open: `into_read` folds the outcome
through `tx::read_outcome` against the session's own `in_tx`, which is one method rather than a
line in each so a third read path inherits the rule instead of having to remember it. What that
prevents is a phantom transaction on the pill — `Poisoned` between transactions on PostgreSQL,
`Open { stmts: 0 }` for a read cancelled before dispatch on either engine — and the reasoning is
under `core::tx`.
**Writing one back is three bindings, one per engine, and each is a decision about the wire rather
than a spelling.** MySQL's `cell_param` sends `MyValue::Bytes` for both `CellEdit::Text` and
`CellEdit::Bytes` — the protocol has one length-prefixed octet string and the server coerces it to
the column — but they arrive there by different routes and only one is reversible: `Text` is the
user's characters encoded as UTF-8, `Bytes` is the octets themselves. Collapsing the two at the
*call site* is what would hurt, because `String::into_bytes` over a lossily-decoded blob is not
the blob. SQLite's `cell_param` binds `rusqlite::types::Value::Blob`, since `Text` and `Blob` are
different storage classes and `BLOB` affinity is the one that never coerces — a blob bound as text
on such a column stores text. PostgreSQL inlines its values, and `pg_cell_lit` (the widened
`pg_opt_lit`) emits `decode('<hex>', 'hex')`, deliberately **not** `'\x…'::bytea`: that form needs
its leading backslash to survive the string literal and reach `bytea`'s input function as that
type's hex marker, which holds only while `standard_conforming_strings` is on — settable per
database and per role, the same dependency `pg::roles` already refuses to take. With it off the
*string* literal claims the backslash first, as its own hex escape: `'\x41424344'` resolves to the
seven characters `A424344` (`\x41` being the escape for `A`), `bytea` then reads those in its
escape format, and the column takes seven bytes of ASCII where four bytes of value belonged. How
that presents depends on the payload — bytes whose escaped form is still valid UTF-8 store the
wrong value with no error at all, and one that is not fails the statement outright, which is how
most real blobs would land. `decode`'s argument is hex
digits and nothing else, so there is nothing in it to escape and nothing to quote; the hex is
lower-case and zero-padded to two digits per byte
(`bytea_hex_is_zero_padded_two_digits_per_byte`), because an odd-length string shifts every byte
after the short one and `decode` rejects it — a failure that would appear on some blobs only.
**That setting is now pinned rather than assumed, for the module's other literals.** `pg_str_lit`
doubles quotes and nothing else, which is correct under `standard_conforming_strings = on` and
only there: with it off PostgreSQL reads a literal as an escape string, `\'` is an escaped quote
rather than a backslash and a quote, and a row's **own text data** ends the literal — after which
the rest of that value is parsed as SQL. `blob_on` was the sharpest of those — a second statement
running from one click on a connection marked read-only — and it is the one that has since moved
to the extended protocol, where `Parse` refuses a second statement outright; the pin still stands
for every other literal this module inlines, which is why it is written down as the belt and not
as the thing that was fixed. The
setting is `USERSET` and settable per database and per role, so the default is no guarantee:
`connect_probe` sends `-c standard_conforming_strings=on` in the connection's `options`, on the
**startup packet**, so it is in force before the first statement and cannot be undone by anything
the app runs — a connection's init SQL included, which `init_sql::check` refuses to let name
it. It is the belt and not the fix — the values still reach the server inside statement
text, and binding them is open work — but it removes a dependency this module already refuses to
take twice, in `roles`' prefix comparison and in `pg_cell_lit`'s argument for `decode`.
**It is also why `run_ddl` here issues nothing where `mysql::run_ddl` issues
`MYSQL_LITERAL_MODE_SQL`**, which is a gap a reader comparing the two will otherwise read as an
omission — it was raised as one. The counterpart would be `SET standard_conforming_strings = on`
after connecting, which is a second mechanism for one guarantee and the weaker of the two: it
leaves the window between connect and the `SET` that a startup option does not have. Every
literal in a plan was written by `export::sql_literal` for exactly the meaning the startup packet
already settled. **One line of connection config with nothing testing it is the shape that gets
deleted as dead configuration**, so two tests hold it, both watched failing with the option
replaced by an `application_name`: `export`'s
`only_mysql_needs_a_statement_to_settle_what_a_literal_means` reads the option out of this file
over code only — the comment above it names the setting, and the first version of that gate
stayed green over the deletion by matching the comment — and the live tier's
`a_password_with_a_backslash_is_stored_as_it_was_typed` sets the GUC **off** on the role the tier
connects as, checks a plan's connection reports `on` regardless, resets a password to `p\ass'q`
through `Db::run_ddl` and logs in with the typed value while the mangled one is refused.
The live tier's `staged_bytes_reach_the_column_as_bytes` runs the whole chain — stage, commit,
re-read — on all three servers, and it is what proves the MySQL binding; it does **not** pin the
PostgreSQL choice, because a server with the default setting takes both forms, which is precisely
why the argument is written down rather than left to the test. SQLite's leg needs no server and
is in `sqlite.rs` (`a_staged_blob_is_written_as_a_blob_and_not_as_text`, which asserts the stored
`typeof()` and not merely the bytes: a payload that happens to be ASCII reads back identically
through either arm, so the storage class is the only thing that tells the wrong write apart), and
`a_blob_column_is_writable_end_to_end_but_never_as_text` composes the four links the pure tests
each pin apart — C2's narrowing, the `DirtyCells` widening, the grouping and the binding.
**A staged `CellEdit::Default` is the one value none of the bindings carries**, because no
parameter can hold the keyword, and the engines disagree on where it may be written. MySQL's
`cell_param` answers `None` for it: `build_update` writes `c = DEFAULT` inline in place of the
`?`, and `build_insert` leaves the column out, which means the same thing
(`a_staged_default_is_written_inline_and_binds_nothing`). PostgreSQL's `pg_cell_lit` renders
`DEFAULT`, legal in both `SET` and `VALUES` (`a_staged_default_is_the_keyword_in_set_and_values`).
**SQLite has neither**: `SET c = DEFAULT` is a syntax error and `VALUES` refuses it, so an
`INSERT` leaves the column out and an `UPDATE` cannot say it at all. `staged_defaults` reads each
staged column's `dflt_value` and `hidden` out of `pragma_table_xinfo` **on the write transaction
itself**, before the first step, and `statement_for(step, &defaults)` writes the declared default
expression parenthesised — `(CURRENT_TIMESTAMP)`, `('new')` and `((6 * 7))` are each one operand
in `SET` — or `NULL` for a column declaring none, which is SQLite's own default for it.
**Except a name, which `default_operand` writes as a string literal** (`export::sql_literal`):
SQLite stores a bare word after `DEFAULT` as its text, and `dflt_value` hands it back as
written, so spliced in as an expression it became a column reference — `DEFAULT active` failed
the commit with *no such column*, and `DEFAULT "status"` beside a `status` column wrote that
column's value. A lone identifier token — a bare word, or one whole `"…"`, `` `…` `` or `[…]`
token — is a name; `CURRENT_TIMESTAMP`, `CURRENT_DATE`, `CURRENT_TIME`, `NULL`, `TRUE` and
`FALSE` are values and are not, and every other form goes in parenthesised as before
(`a_staged_default_written_as_a_name_is_the_string_an_insert_stores`, which asserts the
committed row equals what `INSERT … DEFAULT VALUES` stored). A generated column (`hidden` 2 or 3) and a column the table does not have are `DbError::Refused`
before anything runs, rather than an `UPDATE` SQLite refuses in words about something else
(`a_staged_default_writes_the_declared_default`,
`a_staged_default_on_a_generated_column_is_refused` — both red against a writer that sent the
bare keyword, on SQLite's syntax error at `DEFAULT`). SQL Server's arm is under its own entry.
The live tier's `a_staged_default_writes_the_columns_default` runs against MariaDB, MySQL 8 and
PostgreSQL (`tests/live/writeback.rs`) and against SQL Server 2022 and 2025
(`tests/live/mssql.rs`).
**`Db::run_script` is `stream_query` inverted** — it takes a bounded
`Receiver<script::Statement>` where that one takes a Sender — and it is the load half of the
round trip `dump.rs` opens. It executes each statement in order on one pinned connection, stops
at the first the server refuses, and returns `(script::ExecEnd, ran)` for the driver to fold
against how the *reading* half ended. Three arms, all of them minimal by design: no transaction
of ours on any engine, and no pragma on SQLite. See the connection invariant for why it can be
neither `Session` nor `run_ddl`, and `core::script` for the module it serves. Cancellation is
selected on **while waiting for the next statement** as well as while one runs — a load stalled
on a slow disk spends most of its life waiting, and a Stop that only landed between statements
would look ignored.
**`Db::stream_query` is the whole-table export, and the row cap is the thing it exists to
escape**: it runs `sql` uncapped and hands the rows to a *bounded*
`tokio::sync::mpsc::Sender<ExportChunk>` in blocks of `chunk_rows` as they arrive, returning how
many went out. `ExportChunk` is `Result<ResultSet, String>`, and **the error rides the channel as
well as being returned**, because the writer is on the other end and would otherwise read a
closed channel as "the table ended" and call a half-written file finished. It shares the engine
dispatch with `fetch_query` through the private `Db::run_to` — one connection, one statement, one
destination for its rows — and that destination is `RowDest`: `Capped(n)` for every ordinary
query, `Chunked { chunk, tx, sent }` for a stream, whose `cap()` is `usize::MAX` so no row loop
grows a second branch around the comparison it already makes. **Streaming is a second destination
for the rows, not a second way to read them.** Each engine has exactly one row loop, each the
product of a long argument with its driver, so `RowDest` is threaded through them as a parameter
rather than three more loops to keep in step; every previously-capped call site — `Db::fetch_query`,
every arm of `run_batch`, `explain`, `pg::fetch_table`, `Session::fetch_query` on both backends,
and SQLite's `run_query` — simply passes `&mut RowDest::Capped(n)` and behaves exactly as it did. It *owns* its `Sender`
rather than borrowing one, because SQLite's loop is moved into a `spawn_blocking` that needs
`'static`, and that same split is why there are two flush methods and not one: `flush` awaits
(MySQL, PostgreSQL) while `flush_blocking` calls `blocking_send`, which is right inside
`spawn_blocking` and panics on a runtime thread. All three loops flush the tail
**unconditionally for a row-returning statement, including an empty tail**, since the export's
header comes from the first chunk and a table with no rows has only that block.
**A statement with no result set is a refusal, not an empty export.** All three return *before*
that flush when the statement yields no columns at all (a DML/DDL/utility outcome reports
`affected` instead), so nothing whatever reached the channel — and the writer, seeing no chunk,
produced an empty file and reported it as `Done(0)`. `stream_query` answers
`DbError::QueryFailed(QueryFailure::NoRowsToExport)` — *that statement returns no rows to
export* — when the run comes back with no
columns and nothing sent, and puts it on the channel as well, like every other error here. The
refusal lives in `stream_query` rather than in the caller that happens to be careful — the export
menu never offers such a statement — because this is public API and the next caller may not be
gated the same way. The four tests are in `sqlite.rs`, over in-memory
SQLite (`a_streamed_query_ignores_the_cap_and_arrives_in_bounded_blocks`,
`an_empty_table_still_streams_the_block_that_carries_its_columns`,
`a_failed_stream_sends_its_reason_down_the_channel`,
`a_statement_with_no_result_set_is_refused_rather_than_exported_empty`).
**Uncapped changes what a per-cell cost means, and three things came out of that.** A chunk ends
by rows **or by bytes**: `RowDest::chunk_full(rows, bytes)` also cuts at `CHUNK_BYTE_BUDGET`
(32 MiB, read off `ResultBuilder::text_bytes`), because a block is `chunk × the row width` and
nothing bounded the width — the channel holds two, the loop is filling a third and the writer is
rendering a fourth, so a table of 1 MB documents put ~40 GB in flight against a constant whose own
doc promised "megabytes rather than gigabytes". That was a claim about bytes made by a figure
counted in rows; the only thing that had ever stopped it was the per-column 512 MiB arena ceiling,
and hitting *that* is the loss `export::ExportTally::blanked` now reports. The budget bounds the
arena and not the whole `ResultSet`, whose one offset word per cell is proportional to
`rows × columns` and already bounded by the row count. Then the two type hoists: `db::NumKind` /
`num_kind` / `parse_as` split `parse_typed`'s *decision* from its *parse*, since the decision is a
`to_ascii_uppercase()` (a heap allocation), six `starts_with` probes and, for integers, a
`contains("UNSIGNED")` scan — nothing at all per column and 100M allocations on a 5M × 20 export.
A row loop calls `parse_as` with a kind computed once per column, `parse_typed` stays for the
callers with a single cell to convert, and it **is** the composition of the other two, so the two
spellings cannot drift. `pg::cell_kinds` is the same hoist on PostgreSQL and answers the numeric
kind and the binary flag **together**, once per column, replacing a per-cell
`to_ascii_uppercase()`.
**`Db::fetch_query_enforced` is `fetch_query` for a statement a text gate has already judged, with
the session made to agree with the judgement** — the headless paths, where nobody is at the
keyboard to notice the gate was wrong, and the editor's runs on a read-only connection, whose
`run_verdict` is a text gate too. It takes an `Enforce` down the same `run_to` dispatch, whose
`None` is what every other caller passes, and each variant answers one thing a gate that reads
text cannot know. **`Enforce::ReadOnly` is the effect the text does not show**: `SELECT setval(…)`,
`SELECT lo_unlink(…)`, PostgreSQL's `SELECT 1 INTO newtable` and a `SELECT` of a function whose body
deletes all passed `sql::read_only_reason` and all wrote through the headless read path (see
`core::sql`). PostgreSQL's function allowlist now refuses the three that are calls there, but
`SELECT 1 INTO newtable` still passes, and so does such a function on MySQL/MariaDB, and nothing
that reads text sees a view's body or a trigger. So the session refuses writes — `SET SESSION CHARACTERISTICS AS TRANSACTION READ
ONLY` on PostgreSQL, `SET SESSION TRANSACTION READ ONLY` on MySQL/MariaDB, `PRAGMA query_only = ON`
on SQLite. On PostgreSQL it is the session default rather than a `BEGIN READ ONLY`, so the
statement's own implicit transaction is the read-only one and there is no transaction of ours for
it to sit inside. None of the three can be lifted by the statement it guards: PostgreSQL refuses
`transaction_read_only` once a transaction has taken its snapshot, which a `SELECT` has; a MySQL
transaction's access mode cannot change while it runs, so a stored function cannot undo it from
inside; and a `SELECT` cannot set a pragma. **But a statement can lift it for the statements
after it**, and on the two servers the connection is not always the statement's own: Run All
shares one across its statements, a Manual tab's pinned `Session` across the tab's life, and on
MariaDB a multi-statement text through `fetch_query` is several statements on one connection.
PostgreSQL's `SELECT set_config('default_transaction_read_only', 'off', false)` — a read head
naming no write word — turns the session default off for every later transaction, and a
MySQL/MariaDB stored function running `SET SESSION TRANSACTION READ WRITE` is accepted by both,
MariaDB honouring it for the next statement and MySQL 8 for the next transaction after a commit
(verified live on PostgreSQL 16, MySQL 8.4 and MariaDB 10.11). So the default is where the guard
starts, not what it rests on: each path re-asserts it, or opens a transaction that says `READ
ONLY` in its own words, before anything a lift could reach — `run_batch` and `Session::ensure_tx`
below, and on MySQL/MariaDB `fetch_query` runs an enforced text inside a `START TRANSACTION READ
ONLY` that is never committed (the connection closes, which rolls it back). The live tier's
`a_read_only_batch_cannot_be_lifted_for_a_later_statement`,
`a_read_only_pinned_session_cannot_be_lifted_across_a_commit` and
`a_read_only_text_cannot_lift_itself_for_its_next_statement` hold the three. The editor's text
gate now refuses PostgreSQL's spelling too — `set_config` is off `PG_READ_FUNCTIONS` — but the
MySQL family's is a call to a function the gate cannot read, which is why the session's half has
to hold with no gate at all. **SQL Server has no read-only session or transaction
to ask for**, so its arm is a different kind of thing — a rollback rather than a refusal: the
statement (or the whole batch) runs inside a `BEGIN TRANSACTION` and the connection is dropped
without a commit, which the server rolls back, and since its DDL is transactional a table a
`SELECT … INTO` made goes too. The write *happens* first, so it is briefly visible to a session
reading uncommitted data, and what a rollback cannot return — a procedure's effect outside the
database (`EXEC`, `xp_cmdshell`), a sequence `NEXT VALUE FOR` advanced, anything sent to another
server (`OPENQUERY`, `OPENROWSET`) — rests on the text gate, which refuses each by name
(`core::sql`). A login granted only `SELECT` is the one guard that sees everything, and
`connection::read_only_caveat` puts a sentence saying so under the connection form's Read-only
switch, on SQL Server alone. **What it refuses is a transactional write, and no
more**: on PostgreSQL a function that never calls `PreventCommandIfReadOnly` — a replication slot
dropped or created, `pg_stat_reset` — ran on this very session live on 16.15, so those are the
text gate's to refuse (`core::sql`'s entry) — the editor's on a read-only connection as much as
the headless paths', since `run_verdict` asks `read_only_refusal` of each statement there on
every engine (the write-guard invariant). **`Enforce::AsJudged` is the lexer the gate
assumed.** On a MySQL/MariaDB server whose `sql_mode` carries `NO_BACKSLASH_ESCAPES` or
`ANSI_QUOTES`, `SELECT 'a\'; DELETE FROM t; -- '` is one `SELECT` to the gate and three statements
to the server, and the `DELETE` runs, because `mysql_async` sends multi-statement text whether the
caller wants it or not — `CLIENT_MULTI_STATEMENTS` is hard-coded in 0.37's
`Opts::get_capabilities`, with no toggle. Shown end to end on a throwaway MariaDB 11.8 with the
global mode set: plain `fetch_query` deleted a row, and both `Enforce` modes deleted nothing.
`mysql::enforce_session` runs on **both** variants: it reads `@@SESSION.sql_mode`, strips it
through `core::sql::mysql_mode_lexed_like_the_gate`, sets the result as a **bound parameter** so
nothing the server listed is spliced into SQL text, and **reads it back** through
`mysql_mode_is_lexed_like_the_gate` rather than trusting the `SET` — a combination mode the list
does not know must fail the statement, not run it under a lexer the gate did not use. It then
**reads `@@SESSION.character_set_client` back** and refuses, as a `DbError::Refused`, a set
`core::sql::mysql_charset_is_lexed_like_the_gate` rejects: under `gbk` a backslash can be half
of a character, so the gate's `\'` is the server's closing quote. The handshake asks for
`utf8mb4`, but a server's `init_connect` or a skipped handshake can say otherwise, and `SET NAMES
gbk` as a connection's init SQL had MCP `run_query` delete rows until the init list refused it.
Only then does `ReadOnly` add its `SET SESSION TRANSACTION READ ONLY`. PostgreSQL's `AsJudged` asks nothing,
because the one setting that moves a quote there, `standard_conforming_strings`, is already pinned
on the startup packet by `connect_probe` (above) — the same hazard, closed earlier — and SQLite's
asks nothing because it has no backslash escape to disagree about. SQL Server's asks nothing
either: the gate reads `"…"` as a name, and T-SQL under either `QUOTED_IDENTIFIER` setting ends
that span where the gate does. **Failing to put the session in
the asked-for state fails the statement**: it never runs on a session that was only *meant* to be
guarded. The editor asks for `ReadOnly` on a read-only connection and for `AsJudged` on every
other (`main.rs`'s `session_enforce`), as `schemaic exec` already did — it used to ask for nothing
on a writable one, so its run was plain `fetch_query` and the smuggle above reached the server past
the missing-`WHERE` net and `confirm_writes` (security review S6). On a server whose global mode
holds `ANSI_QUOTES` the editor's session therefore runs without it, and `"x"` is a string there.
**A connection's init SQL runs before all of this**, inside the connect and before the
caller sees the connection, so every enforcement statement here — and SQL Server's `BEGIN
TRANSACTION` — is issued after it and overrides whatever it set; that ordering is why it cannot
lift the transaction guard (the init SQL paragraph below, and the live tier's
`a_read_only_session_with_init_sql_still_refuses_a_write`). It is not why it cannot move the
lexing: no statement here re-sets PostgreSQL's `client_encoding`, so there the init SQL's own
allowlist is what keeps it UTF-8, and on MySQL the list and the character-set read-back both do. `sqlite.rs` carries
`a_read_only_fetch_refuses_a_write_and_still_reads` (the row count afterwards, not just the error,
and a read beside it so a session that refuses everything fails) and
`only_the_read_only_enforcement_refuses_a_write`, which holds `AsJudged` and plain `fetch_query` to
still writing; the server legs are the live tier's `enforced.rs`, below.
**The editor's four other ways to run SQL take the same `Enforce` rather than a second
mechanism.** `Db::stream_query_enforced` is `stream_query` with an `Enforce` down the same
`run_to` — `stream_query` is it with `None` — for the *All rows* export, which re-runs the tab's
statement. `Db::run_batch_enforced` is `run_batch` (which passes `None`) for the auto-commit Run
All, with its one connection put in the state before the first statement: `enforce_session` after
the open on MySQL/MariaDB, `SET SESSION CHARACTERISTICS AS TRANSACTION READ ONLY` after the
connect on PostgreSQL, `PRAGMA query_only = ON` on SQLite. A connection that cannot be put in it
fails the first statement and cancels the rest, exactly as one that cannot connect does. **On the
two servers the read-only default is then re-asserted before every statement** — `SET SESSION
CHARACTERISTICS AS TRANSACTION READ ONLY` / `SET SESSION TRANSACTION READ ONLY` — because the
statement before it may have lifted it (above), and a statement cannot change its own
transaction's mode, so the re-assertion is what holds the next one; a failed re-assertion fails
that statement and cancels the rest. SQLite's `query_only` is set once, a `SELECT` having no way
to set a pragma.
`Session::open_enforced(db, database, enforce)` is `Session::open` (which passes `None`)
with the pinned connection put in the state **before any `BEGIN`**: `mysql::enforce_session`, now
`pub(crate)` for it, on MySQL/MariaDB — a failure disconnects and fails the open — and `SET SESSION
CHARACTERISTICS AS TRANSACTION READ ONLY` on PostgreSQL. **That default is not what the session
rests on**, since a lift inside one of the tab's transactions would reach the next: the session
keeps a `read_only` flag from the open, and `ensure_tx` opens `Ctl::BeginReadOnly` — `BEGIN READ
ONLY` on PostgreSQL, `START TRANSACTION READ ONLY` on MySQL/MariaDB (whose `BEGIN` takes no
characteristics) — so every transaction for the tab's life is read-only in its own words, which
nothing run inside it can undo (`a_read_only_pinned_session_cannot_be_lifted_across_a_commit`).
SQL Server has no read-only transaction to hold, so there the `ReadOnly` open is **refused**
rather than granted an unguarded session (under `mssql.rs`), and `Ctl::BeginReadOnly` is refused
again in `control_on` rather than spelt as a plain `BEGIN TRANSACTION`.
`Db::explain` takes a `read_only` flag, because `EXPLAIN
ANALYZE` *executes* the statement inside a transaction it always rolls back, and the rollback is
not the guarantee it looks like: it undoes an InnoDB write but not a MyISAM or Aria one, and on
PostgreSQL `setval`/`nextval` are never rolled back, so a measured `SELECT setval(…)` moved the
sequence for good. With the flag, `mysql::explain_in_rolled_back_tx` opens `START TRANSACTION READ
ONLY` and `pg::explain` opens `BEGIN READ ONLY`, which refuse both. **Plain `EXPLAIN` takes the
flag too, because planning is not inert**, and this entry used to say it was untouched: MySQL and
MariaDB run a stored function to materialise a derived table or fold a constant while planning,
and PostgreSQL evaluates an `IMMUTABLE` function's call at plan time, which may call a volatile
one. Measured live: on PostgreSQL `EXPLAIN SELECT fold()`, `fold` an `IMMUTABLE` plpgsql function
that `PERFORM`s a deleting one, emptied a table; on MariaDB `EXPLAIN SELECT 1 FROM other WHERE id =
purge_all()` and `EXPLAIN SELECT * FROM (SELECT purge_all() AS x) d` both ran the `DETERMINISTIC`
function and emptied it — so the GUI's Plan on a read-only connection was a write path, and the
MCP server's `explain` would have been one with nobody at the keyboard. So with the flag
`pg::explain` runs the plain form through `fetch_query` with `Enforce::ReadOnly`, and
`mysql::explain` through `fetch_query_enforced(…, Enforce::ReadOnly)`. **That costs the MySQL
family the plan of a write on a read-only connection**: both refuse to plan `EXPLAIN DELETE …`
inside a read-only transaction (error 1792, "Cannot execute statement in a READ ONLY
transaction", true of nothing the user ran), and the transaction cannot be dropped for a write's
plan, since `EXPLAIN DELETE … WHERE id = f()` runs `f` while planning as readily as a `SELECT`
does. `mysql::read_only_plan_refusal` turns 1792 into a `DbError::Refused` that says so, and
passes every other failure through
(`a_refused_plan_of_a_write_says_why_and_other_failures_pass_through`). **It reads the error's
code, not its text** — `server_error_code`, the first `ERROR <code> (` the driver writes ahead of
the server's own text. It matched "1792" anywhere, so `SELECT * FROM t1792` (no such table)
through the MCP `explain` came back as this refusal with its 1146 gone. **And it is worded to be
true from both callers**: the MCP server plans read-only on any connection, so the sentence says
plans *here* run in a read-only transaction rather than that the connection is read-only, and it
points at the `SELECT` form only for a statement `sql::contains_write` calls a write — a `SELECT`
refused here is one whose function writes, and is told so (the live
`a_read_only_plan_refuses_a_write_its_planner_makes` asserts both sentences). PostgreSQL still plans a
write in a read-only transaction, `EXPLAIN` alone never reaching the executor's read-only check.
**Without the flag MySQL/MariaDB still lex as the gate did**: the plain form runs through
`fetch_query_enforced(…, Enforce::AsJudged)` rather than `fetch_query`, and
`explain_in_rolled_back_tx` calls `enforce_session(…, AsJudged)` before its `BEGIN` or `START
TRANSACTION READ ONLY`, on either flag, a failure disconnecting and failing the plan — under a server's
`NO_BACKSLASH_ESCAPES` the text after `EXPLAIN` can be a second statement the gate never saw
(security review S6, the editor's `AsJudged` above).
SQLite's `EXPLAIN QUERY PLAN` takes `Enforce::ReadOnly` as well, for uniformity rather than
need — nothing executes there; `a_read_only_plan_runs_on_a_query_only_connection` holds that the
plan still comes back, and in `id|parent|notused|detail`. SQL Server has
no read-only transaction to open, so `mssql::explain_setup` keeps the guarantee the other way
round: the measured plan runs inside a `BEGIN TRANSACTION` that is never committed, and with the
flag the statement must first pass `sql::read_only_reason` — `DbError::Refused` otherwise —
because that rollback cannot undo a procedure's effect outside the database, a sequence's
advance or a remote write, which are what that gate refuses by name. The estimated plan
(`SET SHOWPLAN_XML ON`) executes nothing and takes no guard (under `mssql.rs`). **What none of
them guards is the person at the keyboard** unticking the connection's read-only box. A typed
`SET SESSION TRANSACTION READ WRITE` reaching the pinned session would lift its default — though
on a read-only connection that spelling is a `Block` first, `SET` being no read head to
`contains_write` — and since `ensure_tx` opens every transaction `READ ONLY` in its own words, the
default it lifted is no longer what the next transaction reads, which `Session::open_enforced`'s
doc says too. The pinned session's state is
decided when `open_session` opens it — so **making the connection read-only, or writable again,
re-pins it**, from
the status-bar toggle as from the form (`Connection::open_session_after`'s `AskFirst`, under
`app/main.rs`).
**MySQL reads one row through two protocols, and `convert_row` has to make them agree.**
`collect_rows` loads a result with `query_iter` (the **text** protocol, every value a `Bytes` that
`parse_typed` keeps the server's own characters of), while `refetch_on` re-reads one row with
`exec_iter` — the **binary** protocol, because a prepared statement with the key bound is what it
is — where MySQL sends `DATETIME` as a `Date`, `TIME` as a `Time` and `FLOAT` as an `f32`. Those
fell through to `MyValue::as_sql`, which renders a *SQL literal* rather than the text form: a
`Date` with a zero time prints as `'YYYY-MM-DD'` and a `Time` as `'{:03}:{:02}:{:02}'`. So on
MariaDB 10.11.14 and MySQL 8.4.11, editing one column of `(1, 'a', '2024-01-15 00:00:00',
'10:30:00', 3.14)` and committing spliced the row back with `2024-01-15`, `010:30:00` and
`3.140000104904175` in three cells the user never touched — measured; re-running the query
restored them, so the grid disagreed with itself about one row and any CSV, clipboard or `INSERT`
export taken in between wrote the wrong text. `binary_as_text` is the translation, and it is a
third arm before the catch-all rather than a fix inside it: a `TIME` is a *duration*, so the day
part folds into the hours and can be negative (`74:00:00`, `-01:02:03`), a bare `DATE` must not
grow a time, and an `f32` goes out through its own shortest round-tripping text because widening
to `f64` first is what produced `3.140000104904175`. The fractional precision comes from
`fractional_scales`, read off the **wire** — the column-definition packet's `decimals` — and not
off the type name, since `type_name_of` builds `DATETIME` from the type code with no precision in
it and a `DATETIME(3)` is indistinguishable from a bare one by name; the binary protocol always
sends microseconds, so without it a `DATETIME(3)` holding `.120` came back with no fraction and a
bare `DATETIME` would have grown six zeroes. It is hoisted per column beside the binary and bit
masks and passed to `convert_row` as a `scale` slice **from every caller**, including the ones
whose values all arrive as `Bytes` and can never reach the typed arm: one function, and a caller
that *could* hit that arm must not be the one that forgot. `refetch_on` reads it before the
collect, which consumes the result and empties `columns_ref`.
**`ZEROFILL` is the same disagreement again, and here the *text* protocol is the side that is
right.** MySQL renders such a column padded to its declared display width, so an
`INT(4) UNSIGNED ZEROFILL` holding 7 arrives as `0007` on a `SELECT` and as `Int(7)` on the
prepared re-fetch — measured on MariaDB 10.11.14 and MySQL 8.4, with `column_length` carrying the
display width. The padding is the value the user is looking at, and the type name is the only
thing `num_kind` is given to decide that by, so `resolve_type_name` takes the wire
`ZEROFILL_FLAG` as a fourth argument and appends ` ZEROFILL` to a numeric type name, and
`num_kind` answers `NumKind::Text` for any name holding it — which keeps the server's own
characters in the grid and in every export, the way `DECIMAL` already does. The binary arm then
has to re-pad rather than parse: `zerofill_widths` reads the width once per column, a sibling of
`fractional_scales` and hoisted for its reason, and `convert_row`'s arms go through
`zerofill_value`, which pads to that width and **never truncates**, a value wider than the
declared width being the server's to render.
**A `FLOAT` or `DOUBLE` is a `ZEROFILL` column too, and only two of the three arms consumed the
width.** MySQL's numeric-type syntax admits the attribute on `FLOAT[(M,D)]`/`DOUBLE[(M,D)]` and
`zerofill_widths` answers `Some` for *every* numeric column carrying the flag, so the width was
computed, carried into `convert_row` and then dropped on the float arm: the binary re-fetch painted
`123.45` over the `0000123.45` the text load had put in the grid, in a cell the user never touched,
and an export taken before the next full re-run wrote the unpadded text out. Measured on MariaDB
10.11.14 and MySQL 8.4.11 — where the `(M,D)` form is deprecated since 8.0.17 and still accepted —
`DOUBLE(10,2) UNSIGNED ZEROFILL` holding `123.45` reads back `0000123.45` over the text protocol
and `FLOAT(8,2) UNSIGNED ZEROFILL` holding `12.5` reads `00012.50`. The scale is part of the
rendering, the server padding to the width *including* the point and the fraction, and
`NOT_FIXED_DEC` (31) — what a float declared without `(M,D)` reports — is handled separately,
since formatting to thirty-one places would invent digits the server never sent; there the
shortest round-tripping form is padded instead. Both callers hand the slice over — the query row loop
and `refetch_on` — for the reason the `scale` slice is passed from every caller. Alignment is
untouched, `Column::is_numeric` reading the leading type token and ignoring the suffixes. Only a
server can send one cell two ways, so the live tier carries it twice: `cases::MYSQL_FAMILY`'s
`int_zerofill`, and a `pad INT(4) UNSIGNED ZEROFILL` column on the MySQL-family legs of
`a_spliced_row_is_the_row_a_fresh_select_would_show`.
`fetch_table_stats` fills `core::stats::SchemaStats` for a whole database — one round trip
either way, and having the set is what feeds the schema tree's size column. It is **lazy and
deliberately not part of `fetch_schema`**: selecting `DATA_LENGTH` from
`information_schema.TABLES` makes MySQL materialize per-table statistics, and the schema fetch
runs on every connect. MySQL adds `STATISTICS` for cardinality and
`performance_schema.table_io_waits_summary_by_index_usage` for index usage — that last one is
routinely off or ungranted, and a failure there leaves every scan count `None`, because zero is
what marks an index unused. SQL Server (`mssql.rs`'s `index_stats_sql`) sums `user_seeks`,
`user_scans` and `user_lookups` from `sys.dm_db_index_usage_stats`, and an index with no row
there reads as zero rather than `None` — the `COALESCE`s in that query — so `is_unused` can flag
it; the count runs from the last restart, not from the index's creation, and a missing row *is*
that counted zero (the server makes the row on first use). That view needs the server-level
`VIEW SERVER (PERFORMANCE) STATE` while the sizes need only the database-level grant, so a
refusal re-reads the indexes without it and leaves every scan count `None` — it used to fail the
whole fetch for a login that could see every size. PostgreSQL reads
`pg_table_size`/`pg_indexes_size`, `reltuples`
(`-1` is "don't know", so it becomes `None`), `n_dead_tup` and the last analyze, through
`query_all_optional` so a restricted `pg_stat_*` can't fail the fetch; a **partitioned parent**
has no storage of its own, so `pg_table_size` on it is a truthful `0` that reads as an empty
table. It was listed with null sizes for that reason, and is now sized as **its leaf partitions'
totals** — table bytes, index bytes and row estimates summed over `pg_partition_tree(c.oid)`'s
`isleaf` rows, the row figure `NULL` while any leaf is unanalysed, since a sum over a `-1` is no
count — while each partition is still listed in its own right
(`the_table_stats_query_sizes_a_partitioned_parent_by_its_partitions`). A sub-partitioned
partition is a parent too and is sized the same way, from its own leaves.
A partitioned **index** (`relkind = 'I'`) carries the same guard in its own spelling, and did not
at first: `pg_relation_size` on one returns `0`, so an index spread over 40 GB of partitions
printed `0 B` in the panel and exported it — the exact reading the sibling query's comment says it
avoids. Its *scan* count is deliberately unguarded: `pg_stat_all_indexes` has no row for such an
index, and NULL there is the truth ("nobody counted"), which is what stops `is_unused` flagging it.
**How much to trust that estimate is `pg_freshness`, lifted out of the fold's closure** — the
three-arm `match (&analyzed, rows)` deciding whether the properties panel captions a row estimate
with "never analyzed", with a timestamp, or with nothing at all. It was the module's one
exception: `pg.rs` extracts this kind of decision everywhere else and says why each time
(`pg_no_default`: "so the rule the flag's contract states has a test standing on it"), while this
one sat inside a `.map(|r| …)` in an `async fn` that owns a `Client`, where nothing could reach
it. Its arms encode two catalogue quirks that are easy to get backwards — `reltuples` is `-1` for
unanalyzed on PG 14+ and `0` on 13 and earlier, and `GREATEST(last_analyze, last_autoanalyze)` is
NULL after a stats reset on a table that *has* been analyzed — so "never analyzed" explains a
*missing* estimate and must never be printed beside a number that contradicts it. All four input
combinations are table-tested, including the freshly `REFRESH`ed materialized view measured on PG
16.15 (`reltuples = 2`, both timestamps NULL). No behaviour changed: this is the seam, not a
defect behind it.
Both builders are string-tested beside `user_schema_filter_excludes_only_postgres_internals` —
that convention exists precisely to catch a guard present in one query and absent from its sibling,
which is what shipped. On the MySQL side the three queries are named constants and the
rows-to-model step is a pure `map_mysql_stats`, so the decisions with a wrong answer that looks
plausible — `MAX(CARDINALITY)` per index rather than per key *position*, the usage view's NULL
index row, a missing usage entry leaving `scans: None` — are unit-tested without a server.
A fourth, `MY_PARTITION_STATS_SQL`, sums each partition's `TABLE_ROWS`, `DATA_LENGTH` and
`INDEX_LENGTH` over `information_schema.PARTITIONS` — a subpartitioned partition's
subpartitions summed into it — and `map_mysql_stats` puts them on `TableStats::partitions`
(under `stats.rs`) through its last parameter (`a_partitions_figures_land_on_its_table`). A
1109/1146 there degrades to no partition figures, as the schema's partition read does (under
`mysql.rs`'s `collect_schema`, below); any other error fails the fetch. Verified live on MariaDB
10.11 (per the change's author): each partition's size and rows matched the catalogue,
subpartitions summed. SQL Server's counterpart is `mssql.rs`'s `PARTITION_STATS`: for each table
with more than one partition, per partition number, the rows of index 0 or 1 and the data and
index bytes from used pages × 8192, as `TABLE_STATS` sums them per table, onto
`TableStats::partitions` under the numbers `mssql_partitioning` names the partitions by (below).
It is a `?` like the queries beside it, so a failure there fails the fetch. Verified live on SQL
Server 2022 (per the change's author): per-partition rows of 71/121/108 and 100/100/50 matched
`sys.dm_db_partition_stats`.
**SQLite answers from the `dbstat` virtual table** (`sqlite::table_stats`), which the bundled
`libsqlite3-sys` build compiles in (`SQLITE_ENABLE_DBSTAT_VTAB`) — this paragraph used to say it
was omitted and that SQLite publishes nothing, which was false, and the dispatcher's arm answered
an empty set inline on the strength of it. One `SELECT name, SUM(pgsize) FROM dbstat WHERE
aggregate = TRUE GROUP BY name` sizes every table b-tree and every index exactly, and what it
cannot say stays `None` rather than a plausible zero: **rows**, because SQLite keeps no estimate
(`sqlite_stat1` samples indexes, and only after an `ANALYZE`), so `count_rows` is still the only
row figure there is; and **scans**, because nothing counts index use, so `is_unused` never flags
one. A non-integer key's automatic index is listed as `PRIMARY` with `is_primary`, as the other
engines print theirs; a rowid table's `INTEGER PRIMARY KEY` and a `WITHOUT ROWID` table's key *are*
the table's b-tree, so neither lists a key index of its own. A view is listed with no figures and a
virtual table has no size, there being no b-tree under its name.
**The cost is the file, so the fetch is capped** — `DBSTAT_CAP_BYTES`, 1 GiB of `page_count ×
page_size`. `dbstat` has no catalogue to read: it walks every page of every b-tree, and this fetch
runs on database expand. Measured on the bundled 3.53, release build, warm cache: 57 ms for a
100 MB file, 606 ms for 1 GB, linear between — so the cap keeps the walk, and the read lock a
writer in another process waits on, near a second; a cold cache costs the disk's read of the
file. Over it the fetch is `DbError::Refused` naming both sizes, so the tree's slot settles at
`Unavailable` and the properties panel shows the sentence in its `Failed` state, Count rows still
offered beneath it (`table_sizes_are_read_from_dbstat`, `table_sizes_are_refused_over_the_cap`).
`fetch_table_stats` joined `ENGINE_ENTRY_POINTS` with it, now that all four modules answer it.
**One fetch per database, shared by all three surfaces.** The properties modal used to issue its
own, on its own connection, on every open — ten tables inspected in a row was ten whole-database
catalogue fetches for figures already in memory, and worse on a server with
`information_schema_stats_expiry = 0`, which re-reads them from the storage engine each time. It
now reads `ConnNode::stats` when that slot holds figures for the active connection's database, and
*warms* the slot when it does have to fetch, so the modal and the tree spare each other in both
directions. Another connection's target still fetches on its own: `db_nodes` is the active
connection's tree, which is why the target carries a `conn_id` at all.
**The warm is `schemaic_ui::warm_stats_slot`, and it reads the slot with `try_with_untracked`.**
It captures a `ConnNode::stats` signal and comes back to it *after an await*, which is longer than
a render: switching connection in between has `nodes_scope_cb` swap in a fresh scope and dispose
the old one, freeing every slot signal — and floem defines `get_untracked` as
`try_get_untracked().unwrap()`, so the read was a panic that takes the window and every tab's
uncommitted edits with it. `DbStatsSlot`'s own rule — *anything that keeps the slot across renders
tracks the list too* — is what this site broke, at a longer range than the rule's wording covers.
`with` rather than `get` for the second reason as well: the question has two answers and
`get_untracked` cloned a whole `SchemaStats` (a `Vec<TableStats>` plus its `HashMap`) to reach it.
Only a **vacant** slot is warmed — `Idle` or `Unavailable`, never `Loading` (a fetch of its own
will land) and never `Loaded` (substituting one reading for another with nothing to say which is
newer). `warming_a_disposed_slot_does_nothing` panics rather than fails against the old body.
**`count_rows` takes a `CancellationToken` like every other unbounded operation.** It was the one
that didn't, and the scan it starts is a full one: closing the modal abandoned the *answer* while
the query ran on for minutes holding a connection, and reopening offered the button again, so N
opens stacked N concurrent scans. Each engine cancels its own way — MySQL `KILL QUERY` from a
second connection, PostgreSQL `cancel_token`, SQLite the interrupt handle, which is why its count
no longer goes through `with_conn` (the handle has to reach the async side before the scan starts).
The app holds one token beside `properties_counting`, cancels it whenever the modal's target
changes (Escape, the ✕, the backdrop, reopening elsewhere) and offers **Cancel** beside the spinner;
a cancelled count is not reported as a failure, since the estimate the user already had is what the
panel goes back to. **That cancel is a variant now and was a sentence** — the count's reporting
closure took `Result<u64, String>` and matched `e == DbError::Cancelled.to_string()`, the one
place in the workspace recognising a typed error by its rendered `Display`, so rewording
`#[error("query cancelled")]` — a copy edit with nothing to fail — would have turned a cancelled
count into a visible error in the panel. The cause was an ordering slip rather than a missing
abstraction: `map_err(|e| e.to_string())` ran in the spawned task, *before* the closure that has
to tell the variants apart, so the closure had nothing left but text. It is
`Result<u64, Option<String>>`, the order the file's two other such sites already had.
**`fetch_schema` takes one too**, and it was the last counter-example to that rule. It reads every
column, index, key, view, check and trigger of a whole database, so on a few hundred tables it runs
for a long time — and the Export modal mounts its `Reading the schema` phase over it behind a full
backdrop whose only exit is a cancel, so Stop did nothing at all and the dump driver's (then
`app/dump.rs`, now `cli/dump.rs`)
`Err(DbError::Cancelled)` arm was unreachable code. Each engine cancels its own way: MySQL races
`collect_schema` against the token and `KILL`s through a second connection, PostgreSQL races the
catalogue reads — split out of the connect into a private `pg::collect_schema` for exactly that, so
the whole sequence is raced rather than a token checked between each of a dozen queries — and calls
`cancel_query` on the server, and SQLite checks **at the door and no further**, the read being
local `sqlite_master` and `PRAGMA` traffic against a file with no server to leave a query running
on. `Db::fetch_schema` refuses at the door as well, before any engine opens anything: a token
cancelled before the call would otherwise pay for a connection handshake and then a *second*
connection to `KILL` a query that was never issued. A caller with no Stop of its own —
`app/main.rs`'s tree refresh — passes `CancellationToken::new()`, which is never cancelled. The
three `mcp.rs` sites that call *this* function used to be in that company and no longer are: each
now hands its token to `mcp::with_deadline`, which cancels it at `QUERY_TIMEOUT` and keeps the
future alive across the cancel so the engine's own KILL branch runs. Nothing there has a Stop
either — what it has is a clock, because the serve loop is sequential and one un-deadlined read
wedges the whole server. `run_ddl` is the schema-editing apply path and is **honest about
atomicity**: PostgreSQL runs the whole plan in one transaction (transactional DDL), MySQL runs
it sequentially and reports which statement failed *and how many already stuck*
(`DdlError::applied`) — every MySQL DDL statement commits implicitly, so a transaction there
would be theatre. **`applied` counts what *applied*, which is not what succeeded**
(`ddl::applied_count` over `ddl::alters_the_database`, whose whole test is that the leading
keyword is not `SET`): a MySQL routine, trigger or event edit is emitted inside a session guard,
and every one of those wrapper statements sets a session variable on a connection the runner
disconnects a line later. Counting them had a rejected `ALTER EVENT` report *2 earlier statements
already applied and cannot be rolled back* when nothing had been — over the app's only disclosure
of a genuinely half-applied migration, which is the one number here that must never be inflated.
The decision is in `core::ddl` rather than a counter in the runner because it is a decision about
emitted SQL and the runner has only strings, which is exactly how the scaffolding came to be
counted; `a_session_guard_is_not_a_statement_that_applied_anything` composes it with the emitters,
so an emitter that starts producing a real `SET` fails there instead of quietly under-counting.
**`DdlError::at` deliberately did not change**: it stays an ordinal over the whole emitted plan,
scaffolding included, because that is the script the preview panel shows and a "statement 3" that
disagreed with what is on screen would be a second wrong number rather than a fix for the first.
**`DdlError::cause` is a value, not the sentence** (`DdlCause`). It was a `message: String`
flattened here, which made the DDL modal's failure the last `DbError` the window could show only
as `to_string()`: the error was finished inside this crate, *query failed:* and all, before
anything could word it, and one English string then served both the modal and the write log.
`Server` is the server's or driver's words as they came (on SQLite with the rebuild's shadow
table named back, `ddl::unshadow`); `Db` a `DbError`, prefix and all — MySQL's every failure and
PostgreSQL's connect, which is why `DbError` is `Clone` now; `Cancelled`; `DanglingForeignKey`
and `BreaksView`, SQLite's two checks after the plan; `NoDatabases`, SQLite's server-level
refusal; and `RollbackFailed`, any of them with the rollback's own failure beside it.
`DdlCause::message` (and `DdlError::message`) is the English, and `Display` is still *statement N
failed: …* plus the *already applied* clause, with two deliberate changes: PostgreSQL's and SQL
Server's Stop said a bare *cancelled* and now say *the plan was cancelled*, and SQLite's
server-level refusal, a `DbError::Query` until then, has lost the *query failed:* in front of a
sentence that was never a query's. The app words it (`ddl_error_shown`, under `app/main.rs`).
**SQLite's DDL is transactional too**, so `sqlite::run_ddl` wraps the whole
plan in one and rolls it back whole, which is why every `DdlError` from that backend carries
`applied: 0` — a half-applied plan is a state this engine never leaves behind, so there is no
partial progress for the report to admit to (`sqlite::ddl_tests`, over in-memory SQLite). That
arm no longer refuses every plan: the gate moved upstream to `ddl::supports_change`, which can
see the `Change` where `run_ddl` has only strings, and refusing wholesale here would have taken
away the drops SQLite genuinely has.
**`run_server_ddl` is the second apply path, and it exists because two statements can take
neither of `run_ddl`'s commitments.** `CREATE DATABASE` and `DROP DATABASE` — the pair
`ddl::is_server_level` marks — cannot run on a connection to the database they are about (one
does not exist yet, the other is what PostgreSQL means by *"cannot drop the currently open
database"*), and PostgreSQL refuses both **inside a transaction block**, which is precisely
what `pg::run_ddl` wraps every plan in. So this path connects without naming the target and
runs untransacted, and nothing is given up by either: a server-level plan is one statement, so
there is no second one for a rollback to protect and no scaffolding for `applied` to discount.
**On MySQL "without naming the target" is `open_serverless`, not `open(None)`**, and the two were
the same spelling for a release. `Db::open(None)` used to mean *"this operation needs no database
scope"*; since a connection gained a configured **Database** it means *"the caller named none, so
use the connection's"* — so `DROP DATABASE shop` on a connection configured for `shop` went out
on a session pointed at its own target, and every later operation answered `ERROR 1049`. The two
readings now have two names (`Scope::Database` / `Scope::Server`), which is the only thing that
keeps them apart: eleven call sites were written under the first reading and most are harmless
only because their SQL happens to be qualified.
The PostgreSQL arm takes the target through `connect_maintenance_avoiding`, since the
*configured* database is the first maintenance candidate and dropping it would otherwise pick,
before anything else, the one connection that cannot perform the statement
(`the_database_being_dropped_is_never_the_one_it_runs_on`, which covers the username guess too
— role and database sharing a name is the default PostgreSQL setup). **The schema tree's Drop
has to hand it the target**, and for a while it did not: its `PlanTarget` carried an empty
`database`, the app turned that into `avoid = None`, and dropping the database a connection is
configured with ran from inside it — *cannot drop the currently open database* (S1-L1-01).
`overlays::drop_database_target` builds it with the dropped name now
(`drop_database_avoids_the_database_it_drops`; live, PostgreSQL 16,
`pg_catalog::a_database_named_by_the_connection_is_dropped_when_avoided`). SQLite's arm is an error
with a sentence in it rather than a statement: a database there is a file, so creating one is
the connection form's business and dropping one would be deleting the user's file off disk.
SQL Server's arm attaches to **`master`**, never the target — which is being made, or must hold
no session, one in it being what makes `DROP DATABASE` refuse as *currently in use* — so it has
no `avoid` to honour, `master` never being a database it creates or drops. T-SQL refuses
`CREATE DATABASE` inside a transaction too (Msg 226, measured), so each statement is its own
batch and commits as it runs, `applied` being the index of the one that failed; Stop sends the
attention, and there is nothing to roll back (`mssql.rs` below).
Which path a plan takes is **decided where the `Change` still is a `Change`** — `preview_of`
stamps `DdlScope` onto the preview from `ddl::is_server_level`, `ddl_preview::apply` passes it
through on the `DdlRunRequest`, and `app/main.rs` branches on it. Asking "is this a `CREATE
DATABASE`?" of a `Vec<String>` would be the hand-rolled scanner this codebase keeps out.
**`DdlPreview::whole` rides the same road and picks between the two in-database runners.** It is
`ChangeSet::runs_whole` — `ddl_is_transactional` and `ddl_rolls_back_as_a_whole` for the engine,
less any change `ddl::touches_natively_compiled_module` answers yes for — and under
`DdlScope::Database` the app takes `run_ddl` when it is true and **`run_ddl_piecewise`** when it
is not. That runner exists because SQL Server refuses every `CREATE`, `ALTER` and `DROP` of a
natively compiled procedure, function or trigger inside a user transaction (Msg 12331, on 2022
and 2025), and `run_ddl` wrapped every SQL Server plan in one — so no edit, create or drop of such
a module could be applied at all. On SQL Server it is `mssql::run_ddl` with `whole: false` (see
`mssql.rs` below); on the other three it delegates to `run_ddl`, MySQL's being piecewise already
and PostgreSQL's and SQLite's transaction strictly more than a caller asking for this is owed.
**What it gives up is said before Apply, not discovered after it**: `ChangeSet::destructive` adds
the sentence that the plan runs a statement at a time, cannot be stopped, and leaves what ran
before a failure applied — on an engine whose plans otherwise run whole, which is why MySQL's
plans never carry it — and the preview offers no Stop, `exit_cancellable` asking `whole` beside
the engine. One answer for the runner, the Stop and the sentence, so the three cannot disagree;
a compare plan's is every set's (`SchemaPlan::runs_whole`), one such module taking the whole plan
out of the transaction. `Change::DropTrigger` gained `natively_compiled` for it, a trigger's
name alone being unable to say (`a_plan_touching_a_natively_compiled_module_does_not_run_whole`,
`a_natively_compiled_module_s_plan_runs_piecewise_and_cannot_be_stopped`; live,
`a_natively_compiled_module_is_edited_created_and_dropped_through_its_plan`, which Azure SQL
Database skips for wanting a memory-optimised filegroup).
`DdlPreview::qualified` is read off the same set and is **not** the same question: it says whether
the plan is *in* a database, and an account change answers `Database` to the first and `false` to
the second — server-wide, but on the in-database runner.
**The refresh branches with it**, and that is not bookkeeping: what a server-level plan changed
is the connection's *list* of databases, so it calls `refresh_schema` (re-list) where an
ordinary plan calls `refresh_db(database)`. Left on the latter, a drop re-introspected a name
that no longer exists and a create re-introspected one the tree had never heard of — both find
no node and return, leaving the tree showing a database that is gone.
**A run from the editor refreshes too.** `run_query_core` and `run_all` call `refresh_db` for the
tab's database when a statement that succeeded `sql::changes_schema` — a `CREATE`, `ALTER` or
`DROP` other than a temporary table's, MySQL's `RENAME TABLE`, PostgreSQL's `COMMENT ON`, SQL
Server's `sp_rename` (bare or qualified), a `SELECT … INTO` a new table, read by each
statement's leading words — each T-SQL statement where it begins (`tsql_statements`), since
`IF OBJECT_ID(…) IS NOT NULL DROP TABLE t` and `SET NOCOUNT ON` ⏎ `CREATE TABLE …` put the DDL
behind another statement's head, and a control-flow piece the splitter keeps whole counts a
`CREATE`/`ALTER`/`DROP` anywhere in it, erring wide — so the tree, the completion
index and the diagnostics catalogue stop showing the schema the run replaced. Both run paths
decide that through `sql::run_changes_schema` (a landed statement that changes it), and the rest
is `sql::schema_settle` — `sql::schema_reload` plus the tree's connection, pure and tested —
which the app's `settle_schema` only performs, at each run that lands and at each end
of the tab's transaction: inside a transaction still open on an engine whose DDL is
transactional (`ddl::ddl_is_transactional`) the reload is **owed** instead (`ddl_owed`, by
tab id), since it reads on a fresh connection that cannot see the change before `COMMIT`; the
transaction's end — the Commit and Rollback buttons (`end_tx`), or a `COMMIT` typed and run —
pays it, a `ROLLBACK`'s too, which costs one introspection and shows the schema as it is. Only
on the tree's own connection, since `refresh_db` reads the active one — and a reload due off it
is **dropped, not owed**: switching back to that connection clears the tree and `load_schema`
re-reads every database fresh, which already shows what was committed, while a debt the
transaction still holds open is kept, since a switch back before the `COMMIT` reads the old
schema (`a_commit_off_the_trees_connection_reloads_nothing_and_owes_nothing`,
`an_open_transactions_debt_survives_the_tree_showing_another_connection`). A filter re-run never
does. `refresh_db` is defined thousands of lines below the run paths (it needs `start_fetch`),
so they call it through `refresh_after_run`, a slot filled once it exists
(`ddl_changes_the_schema_and_data_does_not`,
`ddl_inside_an_open_transaction_waits_for_it_to_end`). A `CREATE`/`DROP DATABASE` from the
editor still needs the SCHEMA header's Refresh.
**An account plan needs a third refresh on top of whichever branch it took.** It runs the
`Database` route (`ddl::is_account_change`), whose refresh re-introspects a *schema* and knows
nothing about `mysql.user` — so a created account never appeared and a dropped one stayed on the
list until the browser was closed and reopened. `app/main.rs` therefore re-fetches the account
list after a successful apply when the browser is open, and puts the selection back to **nothing**
rather than keeping it, since the account it named may be the one that was just dropped.
**It also suspends foreign keys for the duration and checks them before the commit** — `PRAGMA
foreign_keys = OFF` outside the transaction, since SQLite ignores the pragma inside one. It used
to turn them *on*, and enforcing during a plan is not the safe reading it looks like: with them
on, a rebuild's `DROP TABLE` on a parent is an implicit `DELETE FROM parent`, which fires
`ON DELETE CASCADE` and empties the child tables — the table comes back exactly as the user drew
it and another one has quietly lost every row (`sqlite::rebuild_fk_tests` rebuilds a one-row
`artist` and watches both `album` rows behind an `ON DELETE CASCADE` vanish). That is why step 1
of SQLite's own twelve-step procedure turns them off. (The rebuild's statement list carries its
own `ddl::FK_OFF`/`FK_ON` pair for the *other* consumer — see the twelve-step rebuild above; both
are no-ops inside this transaction, which is exactly why this one has to stay.) Nothing is given
up by it: `PRAGMA foreign_key_check` runs against the *finished* state before the commit and
refuses the plan if a reference dangles, naming the child table and the parent so the refusal is
actionable — a stricter question than the per-statement one, since a plan is allowed to pass
through states no single statement could.
**What it must not refuse is what the file arrived with.** That pragma scans the whole database,
and a `.db` written by the sqlite3 CLI — where foreign keys are off by default — very commonly
carries a child row whose parent is gone. Read as the plan's doing, it made adding a column to an
unrelated third table fail with *"the plan leaves a foreign key pointing at nothing"*, and every
DDL operation on that file fail the same way for ever. So the violations are read **before**
`BEGIN` as well, identified by `(table, rowid, parent, fkid)`, and only what
`FkViolations::added_since` reports as new refuses the plan (capped at 10,000 rows, past which it
falls back to comparing counts).
**Step 9 is asked here too, and of the engine.** SQLite's own procedure says to deal with the views
a schema change affects, and a plan has no way to: `legacy_alter_table = ON` — right for the
rename — is exactly what stops the engine noticing, so a rebuild that dropped or renamed a column
a view names *reported success* and the user found out the next time they opened the view. (The
**native** `ALTER TABLE … DROP COLUMN` is refused by the engine for the same case, so this was a
divergence inside one feature.) The authority on what a view resolves to is SQLite, so
`broken_views` asks it: for every view in the catalogue, *prepare* `SELECT * FROM <view>` — a parse
against the schema the plan left behind, no rows read. Read before `BEGIN` as well and compared
(`first_newly_broken_view`), on the same inherited-vs-added reading as the foreign keys: a `.db`
can arrive with a view over a table that is already gone, and refusing for that would take DDL away
from every other table in the file. A plan that broke one is rolled back whole, naming the view and
the engine's own reason (`sqlite::rebuild_bystander_tests`).
**`Db::trigger_source` is the fourth MySQL-vs-MariaDB text divergence** (with `mysql_column`'s
defaults, the view `ALGORITHM` and `mysql_check_clause`) and the only one that is not an
optimisation: `information_schema.TRIGGERS.ACTION_STATEMENT` on MySQL 8 returns the body with
its escapes **already resolved** (`'C:\temp'` → `C:`,0x09,`emp`; `'it''s'` → `'it's'`), and the
damage is *not* recoverable by re-escaping — so a recreate writes a different trigger, or fails
1064 after the `DROP` has committed and destroys it. `SHOW CREATE TRIGGER` is the only faithful
source and carries `sql_mode`/`character_set_client`/`collation_connection` in the same row;
`trigger_body_of` is the pure positional reader beside `view_algorithm_of` (anchored on the
first `FOR EACH ROW` at a *code* position, since a table can be named `` `x FOR EACH ROW y` ``).
**The `FOLLOWS`/`PRECEDES` clause it then steps over is matched on a word boundary, for the same
reason that anchor is a code position.** A bare byte prefix is not the question a keyword asks: a
labelled compound statement is legal SQL and a label may be any identifier, so a body opening
`followsx: BEGIN … END followsx` matched `FOLLOWS`, lost one identifier's worth to the "ordering
clause" and came back **without its first token** — into the editable draft *and* into the diff
baseline, where `TriggerDraft::validate` parses no body and so objected to nothing. An unrelated
edit then emitted `DROP TRIGGER` followed by an invalid `CREATE`, with no transaction between
them: measured on MariaDB 10.11.14, `ERROR 1064`, trigger gone.
`an_ordering_keyword_is_matched_on_a_word_boundary` is the unit pin; the live one, and why it had
to be pointed at this call rather than at introspection, is under `tests/live/` below.
Read **lazily, per trigger**, when the editor opens. **MariaDB takes this round trip too**, and
the two functions' own rustdoc used to say it did not — `Ok(None)` "on MariaDB", "never reaches
here" — while the only gate is `engine != Engine::MySql` and both flavours are `Engine::MySql`.
MariaDB returns everything verbatim, so the second read is *redundant* there rather than skipped,
which is a weaker claim and the true one: the word-boundary defect above was measured on MariaDB
precisely because this parser does run on it.
`mysql_triggers` also gives a group's *leading* trigger a `PRECEDES` anchor: MySQL appends a
no-clause `CREATE TRIGGER` **last**, so replacing the leader silently reversed the firing order.
On the PG side, both of `pg::fetch_schema`'s trigger queries filter `tgparentid = 0` inline (a
partition's cloned trigger is `tgisinternal = false` and can only be dropped through its parent),
and `UPDATE OF` columns + a function's `proconfig` arrive **one row each** rather than
string-aggregated — same rule the enum labels follow, and for the same reason.
**Stored routines** are read with the rest of the schema on both engines that have them.
PG has **three** filters over one reader (`routines_where`), and which one a caller stands on is
the whole design. `routine_scope` is the floor both share: `prokind IN ('f','p')` (an aggregate or
a window function has no body to show and no `CREATE FUNCTION` that would recreate it; the column
is PG 11+, and there is no pre-11 spelling that tells a procedure from an aggregate) in a user
namespace. `routine_filter` adds **not owned by an extension** (`pg_depend.deptype = 'e'`) and is
what the schema tree browses — that exclusion is applied here and not to the types beside it, and
the difference is degree rather than principle: an extension installs a handful of types and
hundreds of functions, and PostGIS alone would bury a database's own routines under ~1000 `st_*`
rows in whichever namespace it was created in. **That exclusion had a second reader nobody had
asked**, which is what `extension_routine_names` is for: the typo checker exempts the names the
tree lists, so a name kept out of the browse list was squiggled as a misspelling under correct SQL
(19 of 196 on PG 16.15 — see `core/schema.rs`'s `extension_routines`). It is one
`SELECT DISTINCT p.proname` scoped by the same `routine_scope`, the exact complement of what
`routine_filter` excludes, `DISTINCT` because an overloaded name has a `pg_proc` row per overload
and the checker only ever asks about the name. No bodies, no arguments, no settings, so the browse
decision above is untouched. **`trigger_function_filter` does not add it**, and
that is not an oversight: a trigger binds to whatever returns `trigger`, extension-owned or not
— `moddatetime` is the standard "touch the modified column" function and arrives exactly that
way — and the picker reading it is a dropdown with no free-text entry, so a function missing from
that list is a function no trigger can be pointed at. It narrows on `prorettype` instead, on the
**server**: filtering in Rust meant shipping every routine body in the database over the wire on
a call the trigger editor makes every time the routine editor closes back to it. Both queries
inside `routines_where` take the same filter string, because the settings fold is keyed on oid
and a filter that disagreed would attach one routine's `SET` clauses to nothing.
MySQL reads `information_schema.ROUTINES` plus `PARAMETERS` — that server publishes no rendered
signature, so `mysql_routines` rebuilds the `IN a INT, OUT b TEXT` form from one row per
parameter, keyed by name **and kind** because a function and a procedure may share a name, and
excluding `ORDINAL_POSITION = 0`, which is a *function's return value* rather than a parameter.
**The `ROUTINES` read binds by the query's own aliases, not by position.** The query is the
`MY_ROUTINES_SQL` const so a test can read it, and `my_routine_row` → `my_routine_row_from` maps
`n`/`ty`/`body`/`sqlmode`/… onto `MyRoutineRow`'s fields. It is a struct rather than the tuple
its siblings here are because fourteen columns is past `mysql_common`'s twelve-element `FromRow`
ceiling — which is the same thing as saying the compiler stopped checking the arity — and the
reader that replaced the tuple indexed `0..=13` against a `SELECT` fifteen hundred lines away
with nothing but a doc comment holding the two in step: insert a column at position 3 and `body`
starts reading `CHARACTER_SET_NAME`, `sql_mode` starts reading `ROUTINE_COMMENT`, the suite stays
green, and what ships is a routine whose Body field shows `utf8mb3` and a recreate built from
that — on the engine whose `DROP` commits on its own. `my_routine_row_from` takes the reader as a
closure so a test can supply one (`mysql_async` doesn't re-export `mysql_common`'s row
constructor), and the test-only `MY_ROUTINE_COLUMNS` is a **third** statement of the list on
purpose: the two that matter are the query and the reader, and checking one against the other
would only be checking whether the same hand wrote both.
`Db::routine_source` is the fifth text divergence and the counterpart of `Db::trigger_source`:
`ROUTINE_DEFINITION` resolves the body's escapes exactly as `ACTION_STATEMENT` does, and every
MySQL routine edit begins with a `DROP` that commits on its own, so `SHOW CREATE` is the only
source an edit may be built from. `routine_body_of` is its pure reader; unlike `trigger_body_of`
it has no keyword to anchor on — a routine has no `FOR EACH ROW` — so it skips the parameter list
as a balanced group and then consumes characteristics by keyword until one isn't. Consuming
greedily is safe because the two vocabularies are disjoint: no MySQL statement begins with
`COMMENT`, `LANGUAGE`, `NOT`, `DETERMINISTIC`, `CONTAINS`, `NO`, `READS`, `MODIFIES`, `SQL`,
`DATA`, `SECURITY`, `DEFINER`, `INVOKER` or `RETURNS`. **A `RETURNS` type's modifiers each take
their argument with them, or take none** — `TYPE_FLAG` for `UNSIGNED`/`ZEROFILL`/…, `TYPE_NAMED`
for `CHARSET x`/`COLLATE x`, and `CHARACTER SET x` matched as the three-word form it is. A
keyword consumed without its value leaves that value at the head of what is returned as the body,
which is the worst failure this reader has: the editor shows a corrupt body and the Apply emits a
1064 *after* the `DROP` has committed. A bare `SET` is on neither list for the same reason from
the other side — it swallowed the first word of a body that legitimately begins `SET @x = 1`.
Both the `Create` column and
`ROUTINE_DEFINITION` are **nullable** — MySQL returns NULL to an account without `SHOW_ROUTINE`
or ownership — and an unread body leaves the editor on whatever the schema fetch carried rather
than blanking it, while the row's session values are returned either way (see `RoutineSource`).
SQLite has neither call: it has no stored routines at all.
**Scheduled events read the same way and degrade differently.** `MY_EVENTS_SQL` /
`my_event_row` → `my_event_row_from` / `MY_EVENT_COLUMNS` are the routine trio again, sixteen
columns instead of fourteen and for the same arity reason; `mysql_events` folds the rows,
deciding the schedule's shape from `EVENT_TYPE` rather than from "is `EXECUTE_AT` NULL" (which
is also NULL for a recurring row whose interval failed to convert), and quoting the timestamps
and the quantity into the SQL expressions the model holds. A `RECURRING` row with no readable
interval becomes `EVERY 1 DAY` rather than being dropped: an event Schemaic can't fully describe
is still one the user must be able to see, rename, disable and drop — and the `ONE TIME` arm
falls back the same way, to `CURRENT_TIMESTAMP`, because an empty `AT` is exactly what
`EventDraft::validate` refuses and would have left Preview disabled for the life of the modal.
Neither fabrication can reach the server on its own: `event_alter_clauses` restates `ON SCHEDULE`
only when it changed, and it hasn't until the user edits the field they are looking at.
Unlike the `TRIGGERS` read
beside it, a **missing `information_schema.EVENTS` degrades to an empty list** on 1109/1146 — the
same two codes `CHECK_CONSTRAINTS` retries on, and for a sharper reason: the MySQL-protocol
servers that aren't MySQL are exactly the ones that may not implement the scheduler, and a
database whose tables can't be browsed because it has no events catalogue is a far worse outcome
than one whose Events folder is empty. `Db::event_source` is the sixth text divergence;
`event_body_of` is its pure reader and is the simplest of the three, since `DO` is a keyword it
can anchor on — what it still needs is `sql::skip_noncode`, for the event named `` `do` `` and
the `COMMENT 'do not touch'` that both sit before the real one.
**Partitions are read in `collect_schema` and degrade as events do.** `MY_PARTITIONS_SQL` reads
`information_schema.PARTITIONS`' rows that carry a partition name — an unpartitioned table has
one row without, left out — ordered by table, partition and then subpartition ordinal, so a
subpartitioned partition comes back once per subpartition; a 1109/1146 leaves the list empty,
for the events read's reason. The pure `mysql_fold_partitions` puts them on
`TableInfo::partitioning` (under `schema.rs`): the key is the method and expression as the
server prints them (`` RANGE (`y`) ``, `` LIST COLUMNS (`region`) ``, `KEY ()`), and each bound
is rebuilt from `PARTITION_DESCRIPTION` the way `SHOW CREATE TABLE` writes it —
`VALUES LESS THAN (2024)`, `VALUES LESS THAN (MAXVALUE,MAXVALUE)` for a `RANGE COLUMNS`, `VALUES
IN (…)`, nothing for `HASH`/`KEY`. **A description of exactly `MAXVALUE` is written bare**
(`VALUES LESS THAN MAXVALUE`), as `SHOW CREATE TABLE` writes it, where every other description
is wrapped in parentheses — the catalogue prints a plain range's `MAXVALUE` alone, and a
`RANGE COLUMNS` one as `MAXVALUE,MAXVALUE`, which takes the parentheses. The subpartitioning
is `SUBPARTITION BY` method and expression plus a `SUBPARTITIONS n` counted off the first
partition's rows, the subpartitions' names not kept
(`partitions_fold_into_the_clauses_the_server_writes`, its rows taken from MariaDB 10.11).
Unlike PostgreSQL's, the fold is not in `fetch_table_list`, so the first-paint list knows no
partitioning until the full read lands. Verified live on MariaDB 10.11 (per the change's
author): `RANGE`, `LIST COLUMNS`, `HASH`, `KEY`, `RANGE COLUMNS` and `RANGE` +
`SUBPARTITION BY HASH` tables read back as these clauses; the dump and restore are under
`dump.rs`.
**A MariaDB sequence's definition and position are read last** (`read_sequences`, after
`mysql_fold_partitions`): per sequence, `SHOW CREATE SEQUENCE db.s` and `SELECT
CAST(next_not_cached_value AS CHAR) FROM db.s`, since no catalogue view holds either, folded by
`SequenceDef::from_show_create` onto `TableInfo::sequence` (under `schema.rs`). **Best-effort per
sequence**: one the login cannot read keeps `None` and its script says it could not be restated,
and a failure here never fails the schema read the tree and the dump share. Like partitions, it
is not in `fetch_table_list`, so the first-paint list holds no definition.
**A database that is not there is an error, not an empty schema.** Every catalogue read above
runs over a connection to *no* database and filters `WHERE TABLE_SCHEMA = ?`, so a missing
database is not a failure there — it matches no rows and reads as a database with nothing in it.
`schemaic compare -c prod -d shop --source-database shop_stagin` (a typo) took that empty side
at its word and printed a script dropping every table in `shop`, exit 0, where PostgreSQL and
SQL Server, which connect *to* the database, failed with exit 4 (review finding S3-L1-01).
`require_database` asks `information_schema.SCHEMATA` first and answers
`DbError::Query("Unknown database '<name>'")` — the server's own text for a `USE` of one — when
the name is absent **or invisible to the login**, and both `collect_schema` (so `fetch_schema`)
and `fetch_table_list` call it before anything else, so the dump, the MCP server's
`list_schema` and the app's tree fail the same way the comparison does. The live tier's
`suite::a_database_that_is_not_there_is_an_error_not_an_empty_schema` holds both reads to an
error on every leg the suite runs; it was red on both MySQL-family legs before the fix.
