# `core/activity.rs`

`activity.rs` — the **Server Activity** panel's model and every decision it makes about a
snapshot of server sessions: no DB, no timer, no UI. `SessionInfo` is engine-neutral by
construction (`id`/`user`/`client`/`database`/`state`/`sql`/`seconds`/`blocked_by`), so a third
server engine only has to produce one. `SessionState` has four variants and **their declaration
order is the attention order** — `rank` is derived from it, with one addition the states can't
express: a session someone is *waiting on* belongs near the top whatever it is doing, because it
is the one about to be killed. `prepare` is the single entry point the app calls on a fresh
fetch: it derives each session's `blocks` list from the `blocked_by` edges (the engines answer
only one direction), **deduplicates both directions**, sorts by rank → longest-standing → id,
then cuts to `MAX_SESSIONS` and returns **whether it cut** — on the same rule as the live
monitor's `trim_log`, since a silently shortened list looks complete and isn't, but as a `bool`
rather than a count. The fetch asks for `MAX_SESSIONS + 1` rows precisely so this can notice,
which means a server holding five hundred and one sessions and one holding four thousand arrive
here identical: any number derived from them is `1`, and "500 sessions · 1 more not shown" in
front of four thousand is a figure that looks precise and is off by three and a half thousand.
**That verdict rides on `ActivitySummary` as a field, not as an argument to one of its
methods**, and `total_label` prints `500+ sessions` off it. It was a parameter to `total_label`
alone, so the total was the only one of the four figures that could say the list behind it had
been cut: a pooled server at 700 connections all idle in transaction rendered
`500+ sessions   500 idle in txn`, the second figure counting a list `prepare` had already
truncated and stating it in the shape and the colour an uncapped server's would wear. `500+` is honest and the `500`
beside it is not, and it is the unqualified one a person is being asked to act on. The other
three go through `state_label(n, word)`, which takes the word from the caller because the
*colour* is the caller's too — the panel draws each figure in the colour that state wears on the
rows below — while the `+` is the one thing that must not be spelled there. The interface words
the noun itself now, so `figure` — the count and its `+`, nothing else — is public for it, and
`total_label`/`state_label` remain as core's English, the wording `activity_panel` is held to.
A zero keeps its
bare shape: the line omits a zero rather than printing `0 blocked` in warning red on a healthy
server, and that the omitted zero is itself unreliable while `truncated` — the rows past the cap
could hold every running session on the box — is a real residue of capping the list and a
different fix, wanting a state tally the *server* computes rather than a caveat on a figure the
panel has decided not to draw. The dedup
is the other half of the same honesty: MySQL answers the blocking graph out of
`performance_schema.data_lock_waits` (or `INNODB_LOCK_WAITS`), which is one row per *lock* pair
rather than per transaction pair, so a holder sitting on both a record lock and a gap lock the
waiter needs reported the same edge twice — "waiting on a lock held by 1148, 1148" on the waiter,
and a "blocks N sessions" on the holder counting locks instead of sessions. PostgreSQL's
`pg_blocking_pids` is already distinct, so doing it here is also what stops the two engines
disagreeing. `render_slice` is the last cut, and a different one: the panel is rebuilt whole on
every poll, so it draws the first `RENDER_CAP` of whatever survived the search and *counts* the
rest (an exact figure — it is measured against the list in hand, not a server total nobody
fetched). **The row→`SessionInfo` folds are here too** — `from_mysql_rows(rows, trx, waits)` over
`MyProcessRow`, `from_pg_rows` over `PgActivityRow` (every `pg_stat_activity` column as the text
`simple_query` hands back) — with the backends reduced to fetching and reshaping.
**`PgActivityRow` is a named struct and its field order is `PG_ACTIVITY_COLUMNS`', which is
also what the query projects.** `db::pg`'s `pg_activity_sql(limit)` builds the outer `SELECT`
from that list, so the query and the fold are one list rather than two; `PgActivityRow::from_slots`
destructures the fetched cells with a slice pattern and **refuses** a row that is not exactly
eight of them. It was `[Option<String>; 8]` filled by `std::array::from_fn(|i| opt(r, i))`,
indexed with bare literals — `text(r, 5)` for the query, `r[7]` for the blockers — against a
`const` SQL string in another crate: `opt` answers `None` past the end of the row, so a
projection that lost or gained a column folded silently into a session whose tail was all
`None` — user empty, state `Idle`, blockers gone — and the panel whose one decision is what to
kill went on working and stopped being true. Nothing failed;
`every_slot_of_an_activity_row_is_the_column_the_query_selects` is what fails now, and it is
written against the constant rather than against a helper that restates the positions. Each fold is a
pile of decisions, not an I/O wrapper: which of MySQL's three result sets wins for a thread,
whether a host becomes an address or nothing, whether a whitespace-only `INFO` is a statement,
that a negative `TIME` clamps before it becomes a sort key, that PostgreSQL keeps a row the role
may not inspect (`pid` and `pg_blocking_pids` stay real, so it is still killable and still a node
in the graph) while dropping the `PG_MASKED_QUERY` sentinel so it is neither classified `Running`
nor drawn as a statement, and that a row whose **pid** won't parse is dropped rather than
admitted under a made-up id — a session `0` renders a full row with a live "Kill session" under
it. None of that was reachable from a test while it sat inside an `async fn` holding a
connection, written out once per engine beside the query that fetched the rows.
The two classifiers are here rather than in the
backends because they are judgements, not queries: `mysql_state(command, trx_state)` is why the
transaction view is joined at all — a `Sleep` thread with a live `INNODB_TRX` row is a client
that ran `BEGIN` and went away holding every row it touched, and `SHOW PROCESSLIST` alone cannot
tell it from an idle pool connection; `pg_state(state, has_query, blocked)` promotes a lock wait
(PostgreSQL reports one as an ordinary `active` backend) and falls back on whether a statement is
visible for a backend the account isn't privileged to inspect. `lock_wait`/`lock_wait_text` pick
the wait worth a banner and write its sentence — deliberately `None` when the holder isn't in
the snapshot, since a banner offering to kill a session that isn't there is worse than no
banner. The sentence's decisions are `lock_wait_said`, which answers a `LockWait` — both ids and
`who()`s, the holder's state, `waited`/`held` as `Age`s (`None` where the
server withheld the age) and `others`, the other holders — whose `Display` is the English
`lock_wait_text` wraps, so the panel can word it (`activity_panel.rs`). It is not called
`lock_wait` because that name is the picker's.
**`is_lock_wait` is the one spelling of the server's `LOCK WAIT` word**, because two readers ask
it: `mysql_state`, which marks the row `Blocked`, and `wait_graph_is_worth_fetching`, which
decides whether the lock-wait query runs at all. Those two disagreeing would be a panel that
marks a session blocked and then never fetches what it is blocked by. The second is an answer
about a table the backend already holds — a thread appears in `INNODB_TRX` as a waiter only
while its `trx_state` is `LOCK WAIT`, so with no such row the wait graph is empty by
construction — and what it saves is a guaranteed round-trip failure per poll on MariaDB; the
measurement and the trade are under `schemaic-db` below.
**`matches_query` matches `SessionInfo::running_sql()`, not `sql`**, and the two differ on
exactly the rows PostgreSQL produces most of: an `idle` backend keeps its last statement in
`pg_stat_activity.query` indefinitely, so `sql` is populated for every connection in a pool
while the row deliberately draws nothing. Filtering on `sql` listed all of them — no statement,
no highlight, nothing on screen saying why they matched — with the one session actually running
the phrase lost among them. MariaDB was immune (`PROCESSLIST.INFO` is NULL on a `Sleep` thread),
so the panel behaved one way per engine, which is the failure `SessionInfo` being "engine-neutral
by construction" exists to prevent. **`IdleInTx` is the same case, and stopping at `Idle` was an
omission**: `pg_stat_activity.query` is kept indefinitely for an `idle in transaction` backend
too, while MariaDB's equivalent — a `Sleep` thread with an open `INNODB_TRX` row — reports `INFO`
NULL and drew nothing, so one server state was two panels again, and through `matches_query` the
divergence reached the search box as well: typing `select` listed every idle-in-transaction
PostgreSQL backend and no MariaDB one. Those are the rows this panel is most about — an
idle-in-transaction holder is what the lock-wait banner points at — so it is the band where
drawing a statement that is *not running* misleads most.
*Copy statement* still reads `sql`: it is the one consumer
that genuinely wants the last statement, and it is a menu entry rather than a match. The state
word it matches is the one the row *shows*, which is the interface's: `matches_query_with`
takes it as a `Fn(SessionState) -> &str`, and `matches_query` is that with `SessionState::label`,
core's English (`search_matches_the_state_word_it_is_given`). The
statement is compared **whitespace-collapsed and without allocating**, through
`text_ops::contains_collapsed_ignore_ascii_case` — `INFO` is the *untruncated* statement where
`SHOW PROCESSLIST` stops at 100 characters, there are up to `MAX_SESSIONS` of them, this test is
last in the `||` chain so a partial word matching nothing yet pays for all of them, and the
filter re-runs on every two-second poll as well as on every keystroke.
**`SessionInfo::sql` is a bounded prefix on both engines, and not by coincidence.** PostgreSQL
truncates `pg_stat_activity.query` itself at `track_activity_query_size` (1 KB by default) while
MySQL's `INFO` is the whole statement, so `MY_PROCESSLIST_SQL` reads it through a `LEFT(INFO, …)`
at `schemaic-db`'s `MY_INFO_MAX`: 40 sessions each running a generated 2 MB multi-row `INSERT` —
the ordinary shape of a bulk loader, which is exactly the load someone opens this panel to
watch — otherwise put ~80 MB of statement text in an "engine-neutral by construction" type,
allocated fresh on every poll and scanned end to end on every keystroke. **The cap is in *bytes*,
so it has to be taken over the binary form.** `INFO` is a character column — `utf8mb3` on both
engines (measured: MariaDB 10.11.14 reports `longtext`/`utf8mb3`, MySQL 8.4.11
`varchar(65535)`/`utf8mb3`) — and `LEFT(str, n)` returns `n` **characters**, so the plain form
admitted up to 3× the constant against a `const` assert written in bytes: 500 rows of non-Latin
text against an asserted 64 MiB is ~96 MB, on the very load the cap is for. Measured on both,
`LENGTH(LEFT(<three CJK characters>, 3))` is `9` plain and `3` under
`CONVERT(… USING binary)` — which is the form the statement uses, converting back to `utf8mb4`
so the driver decodes it and a cut landing mid-character comes back as the replacement character
rather than as invalid UTF-8.
**`SessionInfo::seconds` is `Option<f64>`, and `None` is not zero.** PostgreSQL masks
`state_change`, `query_start` and `backend_start` for a backend the role may not inspect, so
its age arrives NULL; folded to `0.0` it drew as **"0s"**, and a connection open for three
hours claimed to have just arrived on the list a person scans for what has been sitting there.
Every reader has an answer for the unknown rather than a substitute: `age` takes the
`Option` and hands the `None` back, which the panel draws as an em dash, both sorts put `None` last (`None < Some`, which is also what
the query's `NULLS LAST` does), and `lock_wait_text` drops the duration from its sentence
instead of inventing one. `KillKind` is an enum and not a bool because both engines offer both and the
consequences differ in kind: cancelling leaves the session and its transaction alive, while
terminating rolls back and drops the connection; `applies_to` withholds *cancel* from a session
with nothing running, and `kill_confirm(kind, session, dialect)` writes the sentence that names
which one is about to happen. It takes the dialect for **one clause, and it is the clause that
decides whether the user's work survives**: MySQL's `KILL QUERY` leaves the transaction usable
and the client can retry, while PostgreSQL's `pg_cancel_backend` leaves it open but *aborted* —
every later statement fails with `25P02` until someone rolls back, so the uncommitted work is
gone exactly as under a terminate. Telling a PostgreSQL user their transaction "stays open" is
true and useless. `supports_activity` is the capability (**false for SQLite, which is a library in this
process and has no other session to see**) and `supports_kill` is *computed* from it rather than
spelling out a second `!= Sqlite`. `age` keeps whole seconds under a minute because
MySQL's `PROCESSLIST.TIME` has nothing finer, and steps the unit up rather than running the
count on. It answers a value, `Age` — the unit and its two counts — rather than a string,
because the units are words, not marks (`d` is not German): the panel words one through
`i18n::session_age`, and `Age`'s `Display` is the English (`4m 12s`, `2d 07h`) that
`LockWait`'s `Display` and core's tests read. It replaced `format_age`, which returned the
English string and left the interface nothing to translate. The **poll interval is stored per connection** (`IntervalRule`, keyed by `conn_id`
like the colour and favourite rules, in `ui_state.json`): it is not a taste but how hard you
are willing to lean on a particular server, and one global number would carry a laptop's two
seconds onto a production replica on the next switch. `interval_for` clamps on the way out and
`set_interval` on the way in, so the picker can never be left with nothing marked; a choice
equal to the default is still stored, or moving `DEFAULT_POLL_SECS` would silently move every
connection someone had deliberately set to the old one.
**The *Locks* view's model is here too, and is deliberately thinner.** `LockInfo`
(`session`/`object`/`kind`/`mode`/`granted`/`blocked_by`/`killable`) is engine-neutral in its shape, as
`SessionInfo` is, but `kind` and `mode` are the engine's own words — MySQL's `RECORD` and
`X,REC_NOT_GAP`, PostgreSQL's `transactionid` and `ShareLock`, SQL Server's `KEY` and `U` —
shown and never interpreted; `object` is whatever the engine can name the lock on, a table with
its index and key where there are ones, a transaction, a database. `LockList` carries the locks,
the truncation verdict and `contended_only` (MariaDB's, under `schemaic-db` below).
`prepare_locks` is `prepare`'s counterpart: every **waiting** lock first — what is stuck is what
the view is opened for — then by object, session, mode and `kind`, and last by `blocked_by` and
`killable`, which no reader orders by but the merge needs; `blocked_by` deduplicated for the
reason the sessions graph is, the engines answering one row per lock pair; identical rows
merged (`locks.dedup()`), because SQL Server answers a row per lock *owner* and one
`ALTER TABLE`'s schema lock came back twenty times, burying everything else. `dedup` merges
only *adjacent* copies, so the sort has to reach every field — two copies kept apart by a row
differing only in a field it skipped both survived. Merged before the cut, so the copies do
not spend the cap; and a cut at `MAX_LOCKS` whose `bool` is said
rather than hidden. The cap matters more here than
for sessions, since an `UPDATE` of a million rows holds a record lock on each on MySQL.
`lock_matches` searches what the row shows — session, object, kind and mode — and, for a lock
not yet granted, the verb its row reads. `lock_matches_with` takes that verb, so the view
passes `activity-lock-waits-for` ("waits for", German *wartet auf*). It matched `waiting` once,
a word no row shows, so typing what the row said hid it. The verb and the sessions' state word
are folded as words in any script (`word_contains`, over `text_ops::fold_for_search`), so
`LÄUFT` finds *Läuft*. A server's own text keeps ASCII folding. `parse_id_list` is `parse_pid_array` under the name a lock
caller asks for, reading both PostgreSQL's array literal (`{5,9}`) and MySQL's `GROUP_CONCAT`
(`5,9`). The pins are `waiting_locks_lead_and_their_blockers_are_distinct` and
`a_lock_is_found_by_what_its_row_says`; they arrived with the functions they test, so there was
no unfixed tree for them to be watched failing against. `locks_tally` is the view's header
figures, a `LocksTally { total, capped, waiting, waits_capped }` whose `total_text()` and
`waiting_text()` are the English the panel words (under `activity_panel.rs`) — `N locks` and
`M waiting`, the first wearing `+` on a capped list and the second
**only when every row kept is a wait**, since each engine sorts the waits ahead of its limit and
`prepare_locks` again before it cuts, so a capped list of 500 with three waits holds all three;
"3+ waiting" there suggested stuck sessions the list did not show
(`the_locks_tally_marks_the_waits_capped_only_when_they_could_be`).
`mysql_locks_in_information_schema` is which MySQL-family table holds the locks: MariaDB's and
**MySQL 5.7's** `INNODB_LOCKS`, MySQL 8's `data_locks`. Deciding by "is it MariaDB" made the view
an error on every poll on 5.7 and Aurora MySQL 2 (`mysql_5_7_reads_its_locks_where_mariadb_does`).
**`killable` is whether the lock's holder is a client session**, computed by each engine's lock
query with the predicate its sessions query filters on. The server's own processes take locks —
a replica's applier, PostgreSQL's parallel and autovacuum workers, SQL Server's system spids —
and the Sessions view leaves them out because killing one stops replication or fails a different
session's statement; their lock rows stay listed, since they can be what a wait is stuck behind,
and `LockInfo::offers` answers `false` for both kills on them.
**A kill is confirmed from the list it was asked from, as that list is now**, and `KillFrom`
names it so `kill_subject` — what the app's kill handler looks its confirm's subject up through,
given both the sessions snapshot and the current lock list — can tell. From the sessions list (a
row or the lock-wait banner) the subject is the session's row; an id the snapshot no longer
holds is a row that went away between the right-click and the click, and `None` asks nothing.
**From the Locks view the subject never comes from the sessions snapshot.** That list is not
polled while Locks is shown, and SQL Server hands a spid straight back to the next connection
(PostgreSQL reuses pids too), so its row for the id could be an earlier, unrelated session: the
confirm said "alice@app1" over a kill that ended a later ETL connection given the same id. The
subject is a bare `SessionInfo` carrying the id, plus the lock the row showed (`KillSubject`), both
from the current lock list; an id it no longer holds, or holds only `killable`-false, asks
nothing, as a sessions miss does. `KillSubject::confirm` is `kill_confirm` with a closing
sentence naming that lock ("Session 53 holds a X lock on sales.dbo.orders."), which is something
the user can check against the row. `kill_confirm` names a session with neither user nor client
as `session N` rather than through `who()`, whose em dash made the question "Disconnect —?".
`LockInfo::offers` stands in for `applies_to`, which a lock row cannot ask — it carries no
session state: *Cancel query* only on a **waiting** lock, a statement stuck in its wait, and
*Kill session* on any, since a held lock may belong to a session idle in its transaction with
nothing to cancel — and neither on a lock that is not `killable`. The pins are
`a_lock_row_kill_names_the_session_from_the_current_lock_list` (seen failing against the
snapshot lookup), `a_server_process_lock_is_neither_offered_nor_confirmed`,
`a_confirm_for_a_session_known_only_by_id_names_the_id` and
`a_lock_row_offers_a_cancel_only_while_it_waits`.
