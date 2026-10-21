# `core/history.rs`

`history.rs` — query-history model (`push`/`clear_conn`/`preview`/`ago`/`relative_time`),
persisted to `history.json`, capped at `MAX_PER_CONN` (200, was 50) entries per connection.
**Not higher, because both readers walk all of them**: the history panel builds a highlighted
row for every visible entry in a `dyn_container` over `banded_list` — no virtual list — and
rebuilds them on each run and each search keystroke, and completion's `Recall` searches every
stored run of the connection as statements are typed. **A statement carrying a credential is not recorded at all**
(`sql::carries_credential`, asked inside `push` so no caller can skip it): `IDENTIFIED`,
`SET PASSWORD`, a `USER`/`ROLE` statement with `PASSWORD`, and T-SQL's own shapes — `CREATE`/
`ALTER LOGIN … PASSWORD`, `BY PASSWORD` (a master key, certificate or symmetric key), a
`CREDENTIAL … SECRET`, a `PASSWORD` or `…_PASSWORD` word under a `CHANGE`/`START`/`EXEC`
head (MySQL's `SOURCE_PASSWORD`, `START REPLICA … PASSWORD`, a procedure's `@password` /
`@subscriber_password`), and the routines that take a password as an argument
(`sql::SECRET_CALLS`: `sp_addlogin`, `sp_password`, `sp_change_users_login`, the replication
agents, `ENCRYPTBYPASSPHRASE`, and those whose argument is a connection string —
`OPENROWSET` but for `OPENROWSET(BULK …)`, `OPENDATASOURCE`, `sp_addlinkedserver`'s
`@provstr`, PostgreSQL's `dblink`/`dblink_connect`/`dblink_exec` — where the password sits
inside a literal the tokens never read), and three more of that last kind with no
`SECRET_CALLS` entry to find: PostgreSQL's `CREATE`/`ALTER SUBSCRIPTION … CONNECTION '…'`,
a MySQL federated `CREATE`/`ALTER SERVER … OPTIONS (PASSWORD '…')`, and MariaDB's
`PASSWORD('…')`/`OLD_PASSWORD('…')` — as calls, so a `password` column stays recorded. A
statement headed by a `WITH` that opens no CTE and holding `PASSWORD` counts too: it can only
be a principal's options clause cut from its header, as a blank line once cut `CREATE LOGIN
app` ⏎⏎ `WITH PASSWORD = …`. SQL Server
arrived without the T-SQL words, so a typed `ALTER LOGIN … WITH PASSWORD` went to
`history.json` and its `.bak` in the clear; the rule is deliberately not per dialect, since
none of the words means anything else as a statement's shape. **It judges each T-SQL
statement where it begins** (`tsql_statements` over each range), not the range's head:
`scan_bounds` keeps a batch whole from a `DECLARE` on and never cuts inside `BEGIN … END`,
so `DECLARE …; CREATE USER … PASSWORD` and the provisioning script's `IF NOT EXISTS (…)
BEGIN CREATE LOGIN … END` headed no range with the password statement, and were recorded.
An entry is written in **two passes** — `push` when the run
launches, `finish` when it lands (duration, rows, `Outcome`) — because the two moments
answer different questions: an entry has to exist while the query is still running (one the
user cancels, or that the app doesn't outlive, is one they may most want back), and only
the completion knows how it went. `finish` matches on `run_id` — an id the app hands out
per launch from a counter that only goes up — **not** on `(conn_id, sql)`, which identifies
the *statement*: two tabs can have one statement in flight at once, and keyed by statement
the slower, older run overwrote the newer one's result on landing. Keyed by run it finds
nothing, because `push` de-duplicated its entry away. Nothing to update is normal, not an
error. `Outcome` has three states, not two — `Unknown` is what an entry starts in and what
a cancelled run keeps, and the panel then shows no outcome line at all rather than guessing.
Duration is the app's **wall-clock** around the whole call, not `ResultSet::elapsed_ms`: it
is what the user waited through, and it is the same measurement for a failure, which is the
case worth finding (a statement that spent 50 s behind someone else's row lock).
`bucket`/`group_by_recency` are the panel's TODAY / THIS WEEK / EARLIER groups, split on
**elapsed** time rather than calendar days — there is no timezone here, only millis, and a
query run twenty minutes ago shouldn't leave "today" because midnight passed. They share
`relative_time`'s arithmetic on purpose: a row's "3d ago" is read directly under its header,
and a test pins the two together (exactly — the pin accepted either suffix under EARLIER and
was therefore vacuous, since the two functions cross over at the same threshold anyway). That
arithmetic is `ago(ts, now)`, which answers an `Ago` (`JustNow`/`Minutes`/`Hours`/`Days`/
`Weeks`) rather than text, and `relative_time` is `ago(..).english()`, byte for byte what it
wrote before (`ago_is_the_bucket_relative_time_writes`): the value is what lets the interface
say "5m ago" in its own language without re-deriving the buckets.
`push` returns whether it recorded anything, so a credential-bearing statement doesn't cost a
whole atomic rewrite of the file for nothing, and `drop_runs` deletes the entries of runs that
never happened: a batch stops at its first failure and reports the rest `Cancelled` without
dispatching them, so a 60-statement script failing at statement 2 evicted the connection's 50
real entries (the cap then) in favour of 48 that never ran. Deliberately **not** applied to a single run —
one the user cancels *was* dispatched and may have written something.
**A batch does not get to evict history it did not contribute, so the cap is deferred.** Pushing
a Run Everything one statement at a time applied the per-connection cap (then 50) on each, so a
51-statement script cost the connection all 50 of its real entries *plus* its own first
statement before anything ran — and the batch then stopped at statement 1, so `drop_runs`
removed the 50 that never happened and the log came back **completely empty**, from an ordinary
Run Everything, with nothing asked and nothing said. Neither function could see it alone: `push`
knew about one statement and `drop_runs` about the tail, which is why the test is
`a_script_that_stops_early_does_not_empty_the_connections_history` and drives the whole chain.
`push_batch` records the batch whole at launch and `trim` applies the cap in `finish_history`,
once the outcomes are known — the only moment anything can tell a statement that ran from one
that was never sent. The overshoot is bounded by capping the batch's own contribution at
`MAX_PER_CONN`, so a connection holds at most twice that between launch and verdict, and the
statements kept under that bound are the **first** ones, because those are the ones that can
already have run.
**A statement repeated inside one batch keeps its *first* run id**, and that is the same seam
read from the other side. `COMMIT;` twice in a dump is ordinary, and deduplicating within the
batch the way `push` deduplicates across runs replaced the first occurrence's entry with the
last one's — so on `A; B; A;` where `A` succeeded and `B` failed, the surviving entry carried
run 3, which is in the undispatched set, `drop_runs` deleted it, and `finish(run 1)` found
nothing: the `A` that **ran and succeeded** vanished from the log entirely. `finish`'s own doc
reasons about this seam in the opposite direction — "a run whose entry has since been
de-duplicated away simply finds nothing" — and is right there, because the survivor describes a
*newer* run of the same statement; here the survivor was keyed to a run that never happened.
`trim` is over the *whole* store rather than one connection because the caller
applying it knows run ids and not connections, and a cap that is idempotent and total needs
neither; being idempotent is also what lets a batch that fitted report no change and cost no
file write. `remove` is the row
menu's single-entry delete, and its key is **`(conn_id, sql)` because that is the identity
`push` maintains**: `push` drops any earlier entry with the same pair before inserting, so at
most one can be in the log at a time, and `remove` is that predicate read backwards. The
coupling is real and neither function states it alone, which is why the test that holds the
seam is `remove_undoes_a_push_and_a_push_undoes_a_remove` rather than a pin on either side of
it. `run_id` looks like the better key and is not — it is `0` on everything written before it
existed, so it names a *run* and not an entry, and deleting by it would take every legacy row
at once; `ts` is no better, since re-running a statement bubbles its entry to the top with a
fresh one and a menu built before that bump would miss its target. It `retain`s rather than
dropping the first match (`push` cannot make a duplicate, a hand-edited `history.json` can, and
leaving a copy behind reads as the delete having failed) and returns whether anything went, for
`push`'s reason: the app rewrites the whole file on every mutation, so a right-click on a row
that is already gone should not spend that write. `RunResult::loaded`/
`failed` own the rows-vs-`affected` choice and the `rows_capped` rule, and `outcome_line` the
facts line's composition, so both are in core rather than in a view builder: a wrong
`rows_capped` writes a number into a log read long after the grid it came from. That
composition is `outcome_line_with(entry, took, rows)` — which facts, the separators, nothing
counted on a failure — with the row count worded by the caller's `rows(count, capped)` and the
duration by the caller's `took(Took)` (`outcome_line_takes_its_duration_words_from_the_caller`);
`outcome_line` is it with English words and marks (`outcome_line_takes_its_row_words_from_the_caller`).
**A duration is a value, `took(ms) -> Took`** — `Millis` under a second, `Seconds` (still the
milliseconds, written to one decimal by `Took::seconds_text`) under a minute, `Minutes(m, s)`
past one — and `Took`'s `Display` is the English, `48ms`, `1.2s`, `2m 05s`. It is a value
rather than a string because units are words as well as marks: German writes `12,3 s` and
`2 min 05 s`, where remarking core's string gave the interface `1,2s` and `2m 05s`. So
`format_duration`, which is `took(ms).to_string()`, is `pub(crate)` — core's own English, for
`finished_note` and the tests — and no view can reach it to show unremarked. `preview`
clamps at `PREVIEW_MAX` while `matches_query` searches the unclamped text, through
`text_ops::contains_collapsed_ignore_ascii_case` so the collapse the panel draws happens during
the comparison rather than into a `String` first — `max_height` and
`clip()` bound *paint*, not layout, so a multi-MB `INSERT` was laid out whole on every rebuild
of the panel, but clamping what is drawn must not become a decision about what is findable.
**`in_view(entry, scope, query)` is the panel's own question**: `scope` is the connection it
shows, `None` every connection (its *All* tab), and then `matches_query` — so
the switch widens which connections are listed and never what the search box matches
(`the_panel_shows_one_connection_or_every_one_under_the_same_search`). With it on, the panel
walks every connection's entries, so `MAX_PER_CONN` alone bounds that list per connection and
not in total — which is why **`panel_view(entries, scope, query, cap)` is what the panel
lists**: `in_view`'s entries, newest first as stored, cut at `cap` (the panel passes
`MAX_PER_CONN`), and `matched`, how many there were in all. One connection's view is never cut,
**over the cap either** — a Run Everything's batch holds a connection above `MAX_PER_CONN`
until its first verdict, `push_batch`'s bound being twice it, and the *Current* tab then hid
real history under *Showing the newest 200 of 350*
(`a_connection_over_the_cap_during_a_batch_is_listed_whole`);
*All* over ten full connections was 2,000 highlighted rows rebuilt on every run and search
keystroke, ten times what the cap was sized for. The search narrows before the cut, so an old
statement past it is still found by searching
(`the_all_tab_is_cut_at_the_cap_newest_first`).
**`palette_hits(entries, conn_id, visible, needle, cap)` is Find Anywhere's seventh tier through
that same filter**, so the palette finds a past statement exactly when the panel would: the
indices of `conn_id`'s entries that `matches_query` accepts, newest first as the log is kept, at
most `cap` (`PALETTE_HISTORY_CAP`, 20). **One connection's only** — the panel's scope with its
*Current* tab, and the
empty query's, since another connection's SQL, literals and all, has no business in this one's
palette — and an entry whose `database` `visible` refuses is skipped, so a hidden database is
gone from this tier as from every other; an entry with no database is kept. It waits for
`body_search::MIN_TERM_CHARS`, as the body and tab tiers do
(`the_palette_finds_this_connections_matching_statements_newest_first`). Note what sharing the
panel's filter brings with it: `matches_query` folds **ASCII** case only and also matches the
database and tab names, where the tab tier above it folds by the find bar's full rule.
`same_database(dialect, a, b)` answers whether two database names one connection reported name
the same database, by the engine's own rule (`database_names_ignore_case`, an exhaustive match):
exact on PostgreSQL, which keeps `Sales` and `sales` apart, and on MySQL, which does so on a
case-sensitive file system and elsewhere lower-cases every name it lists, so the spellings
agree anyway; ASCII-case-insensitive on SQL Server and on SQLite's attached-schema names. It is
what the editor's remembered predictions ask before offering a run or an open tab from one
database in another (`same_place`, under `completion.rs`), where folding case on every engine
offered PostgreSQL's `Sales` statements in `sales`
(`database_names_compare_by_the_engines_case_rule`). The SQL Server arm is the *default*
collation's answer; nothing consults the server's own, so on one installed case-sensitive
the rule errs towards offering.
`wants_attention` (with `ATTENTION_AFTER_MS`; never for a run the user stopped) is the taskbar
call-back's question, and
`finished_note(Finished, failed, took_ms)` the wording of the desktop notification that can
follow it — *Query finished* / *Script failed* and so on over `Took 12.3s.` through
`format_duration` (`a_finished_note_names_what_ended_and_how_long_it_took`) — the English;
what the notification shows is the app's `notify::finished_note`, which words the same
arguments from the catalogue, its duration through `i18n::took`. **It never
carries the statement**, for this module's credential reason one step further out: a
notification outlives the run in the system's notification history, readable by anyone at the
machine, and the SQL may carry the very values it wrote. `Finished` is four kinds — `Query`
(one statement or Run Everything's batch), `Script`, `Export` (a file or a folder of them),
`Dump` — and whether it failed is the caller's to read off its own outcome type.
