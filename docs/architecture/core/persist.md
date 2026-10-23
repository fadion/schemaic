# `core/persist.rs`

`persist.rs` — the small on-disk state that survives a restart, and the one place that decides
**how a config file is written and how it comes back**. `config_dir` is `%APPDATA%/schemaic` or
`$XDG_CONFIG_HOME`/`~/.config`, and each candidate is filtered through **`usable_base`, because
an empty or relative base is no base**. `std::env::var_os` answers `Some("")` for a variable
that is *set and empty* — what a login script clearing one leaves, and what a container image
or a launcher writes as `export XDG_CONFIG_HOME=` — and `PathBuf::from("").join("schemaic")` is
the **relative** path `schemaic`, resolved against the process's working directory. Launch from
`~/projects/acme`, add three connections, quit, and `connections.json`, `chats.json`,
`history.json` and `tabs.json` are in that git working tree; launch from anywhere else and every
one of them reads absent and falls back to defaults, which presents as "all my connections
vanished". Where the keyring is unavailable it is worse, that being the documented fallback in
which `connections.json` holds **plaintext** database passwords, SSH passwords and key
passphrases — landing in a directory the user may be about to `git add`. The test is `has_root`
and deliberately not `is_absolute`: the question is whether the path depends on the working
directory, so a rooted Windows path with no drive letter (`\schemaic`) passes, a Unix-shaped
`HOME` handed to a Windows build by Git Bash passes, and drive-relative `C:schemaic` — exactly
the shape to refuse — does not. The freedesktop spec says an empty `$XDG_CONFIG_HOME` must be
treated as unset; the same reasoning is applied to `HOME` and `APPDATA`, and falling through to
the next candidate and finally to `None` is what the callers already handle.
`UiState`/`ui_state.json` is the original tenant and
`tabs.json`, `connections.json`, `history.json`, `chats.json`, `snippets.json`,
`favorites.json`, `db_colors.json`, `diagrams.json`, `search_history.json`, `accepted.json`,
`ssh_known_hosts.json` and `closed_tabs.json` all arrived behind it. Every decision in it is pure and testable because
**the filesystem is an argument**: `FileStore` is the seam `write_bytes` and `read_bytes` take,
the way `secrets::SecretStore` does for the keyring and for the same reason. What had no test
was never `classify` or `sibling` but the *composition* — that what a save produces is what a
load can recover from — and that composition is what decides whether a user's connections, tabs
and history survive a crash or a full disk, including the failures a real disk will not stage on
demand.
**The save ordering.** `write_bytes` stages the new bytes in a `.tmp` sibling, **renames the
previous generation to `.bak`**, renames the `.tmp` over the target, and falls back to a direct
write only when that rename is refused — putting the moved primary back if the direct write
fails too. A rename, not a copy, because the `.bak` is the crash recovery for the ordinary save
(below) only while it is durable: re-writing the old bytes into `.bak`, as this did until the
v0.34.0 review, truncated the one durable older copy and put the same bytes back unsynced, in
the very window the new primary's contents were still in flight, so a power loss there took
both and the store loaded as defaults. `a_crash_after_an_ordinary_save_still_reads_a_generation`
pins it over `CrashFs`, a store that keeps names and renames across a crash and contents only
when synced. **Only a settled primary is rotated** (`FileStore::settled`): a second save
inside the writeback window found the first's contents still in flight, and renaming them over
the `.bak` unlinked the one durable generation — two ordinary saves and a power loss read the
defaults (`two_saves_inside_the_writeback_window_still_leave_a_generation`). An unsettled
primary is replaced without being rotated, so the `.bak` stays the older, durable generation,
and no fsync is paid per save — until that `.bak` is older than `BACKUP_MAX_AGE` (5 minutes,
`FileStore::age`) or missing, when the save syncs the primary once and rotates it, so saves
seconds apart for an hour of typing do not keep the session's first generation as the backup
(`a_stale_backup_is_refreshed_by_syncing_the_primary_once`). `Fs` answers from a process-wide record of the files it wrote
unsynced (`FileStore::unsettle`, cleared by `sync`): settled once `SETTLE_AFTER` (60 s, past
Linux's dirty expiry and Windows' lazy writer) has gone by, and a file this process never wrote
always; `CrashFs` answers exactly. The two partial failures are pinned over `FakeFs`'s per-path
`rename_fails_from` (`a_save_that_cannot_land_moves_the_primary_back`,
`a_primary_that_cannot_be_moved_is_copied_and_the_save_lands`). Between the two renames the
primary is absent, which `recover` reads as `.tmp`,
then `.bak`. A renamed file keeps its mode, so `FileStore::narrow` takes the `.bak` to 0600 (a
0644 left by a build from before `write_private`); a primary that cannot be moved — absent on a
first save, or held open elsewhere — keeps the old copy-in-place, so a save is never refused
over its backup.
**`Saving { Replacing, Erasing }` is an argument to that save, because every deletion this app
confirms was undone at rest without it.** The history panel's trash asks *"Delete N recorded
queries for this connection? This can't be undone"*, and the save that answered Yes copied the
pre-clear file to `history.json.bak` on its way past — so every statement the user had just
confirmed the deletion of sat in a sibling until the *next* save of that store, which needs
another query run; a clear followed by a quit left it there indefinitely. The user's own SQL is
content this module already treats as sensitive (`open_private_append` narrows the log for
exactly that reason) and the Settings modal's **Log file** row is an explicit invitation to open
that folder and share it. An erasing save takes no `.bak` and removes the one it found **after**
the new file lands.
**And it syncs before it removes it, which is the one place this module pays for an fsync.** The
rename publishes the *name*: on XFS, btrfs, ZFS, an SMB/NFS share and NTFS that metadata can
reach stable storage while the contents have not, so a crash in the window leaves a zero-length
primary — which `serde_json::from_slice` refuses, `read_bytes` reports as `Load::Corrupt`, and
recovers from the backup, the file that branch had just deleted. **The asymmetry is the design
and is why the fix is narrow**: `Saving::Replacing` keeps the previous generation, so the same
window is already covered and the ordinary path pays for nothing — one fsync per
keystroke-driven save is a cost this store does not need. `Saving::Erasing` is the one save with
no second copy, because removing it is the point, so it closes the window itself: one fsync per
*deletion*, a connection removed or a history row deleted. Without it the worst case is deleting
one connection losing all of them, on `connections.json` — the file this entry's own prose calls
the only config file with no second copy anywhere. `FileStore::sync(path)` is the fifth method
of the seam, and the real `Fs` opens the file and `sync_data`s **best effort**, on the same
terms as every other sweep here — a file that cannot be opened has nothing to flush, and
failing the save over it would discard a write the user has already been told about. It is
asserted as an **order rather than an outcome**, which is why the fake store grew an op log:
both orderings end with the same set of files, so a store recording only the final state cannot
tell them apart.
`an_erasing_save_syncs_before_it_removes_the_backup` covers three things — the erasing path
syncs first, the replacing path still does not sync at all, and the credential store's extra
`.corrupt` sweep is after the sync too, which `write_secret_store`'s own doc already said it
owed.
**The sibling an erasing save removes is the `.bak`, and only the `.bak`** — the other one is a
different kind of file, and the difference is the whole of the rule. The `.bak` is the previous
generation of the store being written, so everything in it except the erased row is already in
the new primary and removing it loses exactly what the user asked to lose; every `Saving::Erasing`
save still reaches it. The `.corrupt` is an orphan from a generation that *broke*: `read_bytes`
renames an unparseable primary aside *whole* and the primary now being written came back off the
`.bak`, which is one generation **older**, so the `.corrupt` can hold rows the primary never had
— and `recovery_notice` has already told the user, by name, to go and read them out of it. Unlike
the `.bak`, which is a silent copy and the whole reason `Saving::Erasing` exists, that file was
announced in a modal, so the privacy argument that justifies scrubbing the `.bak` does not carry
across. For a while every erasing save removed both, and **every erasing call site is a *per-row*
erasure** — one snippet, one chat, one history row, a deleted connection's cascade across
fourteen stores — never an erasure of the whole store (the one exception now is `closed_tabs.json`
emptied while *restore tabs* is off, below), so deleting one snippet destroyed
`snippets.json.corrupt` and with it the snippets that existed only there, with no message, as the
side effect of deleting a *different* row
(`an_erasing_save_keeps_a_corrupt_sibling_it_did_not_write`, and the composition in
`a_recovery_then_a_one_row_erasure_keeps_the_rows_it_did_not_erase`).
**`connections.json` is the one store where the scrub still wins**, because under the no-keyring
fallback that copy holds the DB password, the SSH password and the key passphrase in the clear,
in a file that by definition cannot be parsed and therefore cannot be rewritten without them.
`remove_secret_siblings` is that two-sibling scrub — the old `remove_erased_siblings` under a
name that says which stores it is for — and it has two callers, both `connections.json`:
`clear_connections_backup`, the scrub the secret layer runs right after a migration save when the
`.bak` still holds the plaintext that migration just moved into the keyring
(`clearing_the_siblings_reaches_both_of_them`), and `write_secret_store`, whose only production
caller is `save_connections`. **`write_secret_store` is a function beside `write_bytes` rather
than a third `Saving` arm**, because the question is about the *store* and `write_bytes` is
deliberately store-blind: it takes a `&dyn FileStore` and a `&Path` and cannot tell
`connections.json` from `snippets.json`, and asking it to decide is how the blanket rule arose in
the first place. It takes the `FileStore` as an argument like everything else here, so it is
testable without a disk, and `write_bytes` now answers **whether the new content landed** so a
caller with more to sweep inherits the same land-before-you-sweep ordering the `.bak` has
(`deleting_a_connection_sweeps_the_corrupt_sibling_too`, with
`a_credential_erasure_that_cannot_stage_keeps_both_siblings` holding the ordering, and
`a_recovery_then_an_erasure_leaves_nothing_of_the_erased_row` now run on the credential path,
which is where the "This can't be undone" claim is still made about the whole directory).
**A promise the app breaks by design is written as the qualified thing it is.**
`corrupt_sibling_is_swept(path)` answers whether a later save will take this store's `.corrupt`
out from under the user — true for `connections.json` alone — and `recovery_notice` asks it and
records the answer as the notice's `swept`, which says a second sentence for that store: the copy can hold passwords in the clear, so it is
removed the next time you delete a connection, recover anything you need from it first. The other
stores' notice must **not** carry that warning, because for them it is now false
(`only_the_credential_store_warns_that_its_corrupt_copy_is_temporary`). **`write_secret_store`
asks the same predicate before it sweeps**, rather than letting the choice of function settle
it, so the sweep and the sentence describing it cannot come apart: a second secret store routed
through that function without being added to the predicate gets the ordinary notice *and* the
ordinary treatment, not one of each
(`a_store_the_notice_makes_no_promise_about_keeps_its_corrupt_sibling` is the composition, and it
is the pin that fails if the predicate is dropped from either reader). It is a predicate rather
than the test written inline into the notice because the scrub and the sentence describing it sit
500 lines apart and have drifted once already — the shipped version had the scrub and no sentence
at all — and `CONNECTIONS_FILE` is the one spelling `connections_path` and the predicate both
read.
Erasing is a property of the save and not of a particular store, which is why it
is a parameter rather than a `clear_*_backup` per file; `save_json_erasing` is the public entry,
and the erasing callers are the history panel's trash, one history row removed from its menu, a
deleted connection's history, chats and snippets, the AI panel's New chat, one snippet deleted
from the snippet panel, the connection delete itself, and the reopen ring's `closed_tabs.json`
on that delete and whenever *restore tabs* is off. **`write_json_store(file, value,
saving)` is the third entry point beside the two fixed-verb ones**, and it exists because some
stores are written by a single shared closure that both a menu upsert and the connection delete
reach: the colour, favourite, formatter and `ui_state` savers are `Rc<dyn Fn(persist::Saving)>`,
and neither `save_json` nor `save_json_erasing` can serve a closure whose verb is the caller's.
It is the same `write_json` with the argument passed through rather than written in, not a third
policy. The alternative — the closure deciding for itself — is how those stores came to keep a
deleted connection's databases, tables and columns in their `.bak` siblings under a confirm
saying they could not be recovered. **The connection delete erases *all* of its stores now**; it
erased four and the other five were ordinary saves, so `ui_state.json.bak` kept the deleted
connection's expanded tree and hidden databases, `db_colors.json.bak` and `favorites.json.bak`
its databases and tables, `format.json.bak` its columns, and `diagrams.json.bak` its ER-diagram
layout — three of them named in `Saving`'s own doc as having exactly this property. The four
that were right were the four whose save already took a `Saving`; the five that were wrong were
the five written by a shared `Fn()` with nowhere to say it. Three of those five — the colour,
favourite and formatter savers — are built in one place now, `app/ui_stores.rs`'s
`saver(file, build)`, which passes the argument straight through to `write_json_store` and so
keeps no policy of its own. They are no longer on `Ui` at all: each is sealed with its signal
inside a `ui/stored.rs` `Stored`, whose `update` passes `Replacing` and whose `erase` passes
`Erasing`; the app's `save_ui` takes one directly. The delete closure's own gate asserts
exactly thirteen erasing saves in it — an equality, since a floor of ten stayed green with
`table_colors.erase` deleted — **three of the thirteen delegated**, since the history store's
prune moved to `app/history_store.rs` and the snippet store's to `app/snippet_store.rs`, the
closure holding only the call to each, whose erase is gated in the module it moved to, and the
reopen ring's `closed_tabs.json` is `(save_closed)(true)`, erasing by its argument — and
counts the five `Stored` stores' `.erase(` calls toward the same total.
**The tenth is the Find-Anywhere search history, and it was pruned in that closure while being
saved nowhere in it** — its persistence was a `create_effect` at the top of `main` calling
`persist::save_json`, so the prune notified the effect, the effect saved `Replacing`, and the
*pre-prune* file went to `search_history.json.bak` whole. It has an explicit
`Rc<dyn Fn(persist::Saving)>` saver now, on `save_db_favorites`' shape, with the effect keeping
`Replacing` for an ordinary activation, which is the right recovery for adding a row.
`persist_chat` takes a `Saving`, so the
caller says whether a turn finished or a transcript was replaced with nothing; the snippet
library's save says the same thing, now through `snippet_store`'s private `save(snippets,
saving)` rather than the `save_snippets` closure `main.rs` used to hold; and
`persist::save_connections` takes one, with `conn::secrets::save_connections` and `persist_conns`
forwarding it. **Every connection save but the delete stays `Replacing`**, because
`connections.json` is the one config file with no second copy anywhere and losing it loses every
connection — but an ordinary save on the delete path copies the pre-delete generation aside, so
the removed row's host, port, user, database and SSH coordinates (and, with no working keyring,
its three plaintext secrets) land in `connections.json.bak` at the moment the user confirms a
modal telling them the opposite.
**`ConnectionsFile::highest_id` is the other thing a delete leaves behind, and it is a
high-water mark that only rises.** `Connection::next_id` answers "past every id *in use*", whose
own doc explains that an id freed by a delete stays free "because whatever still refers to it
would otherwise be adopted by the next connection created" — true of a gap in the middle and
false at the top, where deleting the highest-numbered connection lowers the maximum and the very
next connection created takes the id just released. The id is the whole of the keyring account
string (`conn.{id}.password` and its three siblings), so that connection hydrates the dead one's
password, SSH password and key passphrase, shows a filled mask, and sends server A's credential
to server B. The delete path already asks the keyring to forget them and says so when it could
not — but a notice is a disclosure, not a barrier. `Connection::next_id_after(existing,
high_water)` is the barrier, and every mint site in `main.rs` takes it
(`every_minted_connection_id_is_past_the_high_water_mark` forbids the bare `next_id` there and
checks the mark is actually written). The field is `#[serde(default)]`, so a file written before
it reads `0` and the answer is exactly the old one — the guarantee begins at the first save
rather than being claimed for ids handed out before it existed — and the app seeds the signal to
at least the maximum in the file, so an older `connections.json` still cannot hand back an id
that is in use.
**`migrate_legacy_once` is the one statement of the legacy-field bargain, and the half it
returns is the whole point of it.** `UiState` carries two legacy/replacement pairs —
`expanded`/`expanded_rules` and `hidden_dbs`/`hidden_db_rules` — each *kept* as a field rather
than removed, because an upgrade must not lose the setting and a `Vec<String>` arriving where a
`Vec<Rule>` is expected fails the whole file's parse. `main.rs` had the same fourteen lines
written out twice for them, with the reasoning split across two comments each pointing at the
other; this is that decision once, generic over the rule because `expanded::migrate_flat` and
`db_hidden::migrate_flat` differ in nothing else (`fn(&[String], &[u64]) -> Option<Vec<R>>`). It
answers `(rules, legacy still pending)`, and the second element is the reason it exists at all:
the migration is refused when there are no connection ids to key by — a
`migrate_flat` `None` means *not yet*, which is a different answer from *nothing to do* — and
the legacy list then comes back **whole**, so `save_ui` writes it out unchanged and a later
launch can still migrate it. Clearing it there is the original bug: a session whose
`connections.json` did not load, or a user who had just deleted their last connection, had the
flat list emptied by the first save and every previously hidden database turned permanently
visible. The migration is refused a second way, when `stored` is already non-empty, or a file
written by a version that kept both fields has every rule duplicated on each launch.
**Its four tests are the reason it is a function at all**, rather than the deduplication.
`db_hidden` already had `no_connections_is_not_yet_rather_than_nothing_to_migrate` and `expanded`
had no migration test at all — and that one tests `migrate_flat` *in isolation*, while the bug
lived in its composition with the caller that cleared the list anyway. The four are
`a_legacy_list_survives_a_migration_that_could_not_run`,
`a_legacy_list_is_consumed_exactly_once`,
`stored_rules_are_never_overwritten_by_a_second_migration` and
`an_empty_legacy_list_is_nothing_to_do_not_not_yet`; the first was watched failing against the
`None => (stored, Vec::new())` this function exists to rule out.
**`schema_filter` is one `String`, not a rule list, because there is one box.** The tree's
filter runs over every connection's `db_nodes` at once, so what is left in it is a single fact
about the panel rather than one per connection; the `Vec<Rule>`-keyed-by-`conn_id` shape the
expansion, hidden-database, colour and favourite stores share does not fit it, and a
per-connection key would also have had to join `delete_conn_now`'s prune census for nothing.
`#[serde(default)]` reads an older file as empty, which filters nothing
(`the_schema_filter_round_trips_and_defaults_to_none`). It changes per keystroke, so it is saved
on a debounce and again at quit rather than by the effects the other fields share (*app/main.rs*,
beside the `WindowClosed` flush).
**`ClosedTabsFile` is the reopen ring across a restart** — `closed_tabs.json`, at most
`CLOSED_TABS_CAP` (10) `ClosedTabEntry`s, most recent first, so Ctrl+Shift+T and the palette's
*Reopen Closed Tab* reach a tab closed before the last quit. An entry is a `SavedTab` under
`#[serde(flatten)]` beside its `label`, the *Query N* number (`0` for none, which a reopen
numbers afresh rather than restoring), so it is the same flat shape `tabs.json` writes and a
file tab comes back the way a restored open one does: the file's contents never stored, only
`file_dirty` (`a_closed_tab_round_trips_as_a_saved_tab_and_its_label`). `capped()` holds a
hand-edited or future file to the ring's length on read
(`a_closed_tabs_file_reads_no_longer_than_the_ring`). **It is written only while *restore
tabs* is on**, and emptied with no `.bak` while it is off: it is the text of tabs the user
*closed*, and keeping it against their wish to start fresh is the wrong default.
`ClosedTabsFile::for_disk(restore, ring)` is what may be written: nothing while the setting is
off, and **never an entry whose text `sql::carries_credential` flags** under any dialect — the
rule `history.json` keeps, so closing a tab that ran `ALTER USER … IDENTIFIED BY` does not leave
the password on disk until ten more closes push it out; the entry stays in the in-memory ring
(`a_closed_tab_carrying_a_credential_stays_off_disk`). `closed_ring_saving(file_empty, on_disk,
erasing, restore)` decides how a write is saved, or that it is skipped — `on_disk` being what
the last write left (`RingOnDisk`, from `ring_on_disk_after`): an ordinary write of a file the
last write already emptied is skipped, an **erasing one never is**, because the ordinary write
that emptied it kept the ring before it as `.bak`
(`an_erasing_ring_write_happens_even_when_the_file_is_already_empty`), **and nor is the
setting's own where that `.bak` was kept**: off, the closed tabs' text is what it says is not
kept, so the write goes ahead erasing — a reopen that emptied the ring before the setting went
off left the text in `.bak` indefinitely
(`restore_tabs_off_after_an_ordinary_emptying_write_erases_the_backup`). The writer is the app's
`save_closed`, under the session-restore passage in *app/main.rs*.
**`ai_harness` has two defaults, and the difference between them is the point.** A file *without*
the key takes the serde default, `default_ai_harness` — `"claude"`, which is what every file
written before multiple harnesses meant, since that was the one CLI the app drove; an upgrade must
not switch off a panel somebody is using
(`a_ui_state_from_before_multiple_harnesses_loads_as_claude`). `UiState::default()` takes
`first_run_ai_harness` — `AI_DISABLED_KEY`, the assistant off — so a new install sends nothing to
any agent until the user picks one (`a_first_run_starts_with_the_assistant_disabled`;
`a_disabled_assistant_round_trips` holds the key through serde and `ai_harness_to_persist`). Off
is therefore also what anything else that lands on `UiState::default()` gets: no config
directory, or — per the paragraph below — a `ui_state.json` that is corrupt with no `.bak` that
parses. Pointing the serde default at `first_run_ai_harness` as a tidy-up would switch
every upgraded install's assistant off.
**An absent primary is not a first run**, and treating it as one was silent data loss.
`recover(primary, staged, backup)` returns the value *and* a `Recovered` — `Primary`,
`FirstRun`, `Corrupt(err)` or `Restored(sibling)` — and an absent primary walks `.tmp` then
`.bak` before it concedes. That order, because `write_bytes` stages the *new* value into `.tmp`
and moves the *old* primary to `.bak`, so where both survive the staged one is the newer; a
sibling that does not parse is stepped over rather than believed, which is what stops a
half-written `.tmp` shadowing an intact `.bak`. Before this the absent case defaulted with
nothing said, the next save wrote the defaults over the primary, and the save *after* that
copied the now-defaulted primary onto `.bak` — the last copy gone within two ordinary saves, and
saves are frequent (a tab change, a history entry). The three ways to arrive there are a crash
between the two renames of **any** rotating save — the primary moved to `.bak`, the `.tmp` not
yet over it, the most frequent of the three — a crash between the staged write and the rename
on a *first* save, which leaves a `.tmp` holding
everything, and the file being removed from outside: the Settings modal's own *Open folder*
button puts the user in that directory, and a roaming-profile sync, a backup restore or an AV
quarantine reach it too. **`read_bytes` classifies before it sweeps**, and sweeps the orphan
`.tmp` only behind a healthy primary — the sweep used to run first and unconditionally, so on
the one load where that copy was the only one the loader destroyed it and then reported a first
run. A **corrupt** primary still consults `.bak` alone and is preserved as `.corrupt`; widening
that arm is a separate decision from the one the absent arm is about, and tests pin it.
**`queue_notice` is the shared startup-notice channel**, over the `RECOVERIES` list the
config-recovery modal drains with `take_recoveries`. Everything loaded before the window is
drawn has the same problem — there is no surface yet to say anything on — so one channel beats
each loader inventing its own, and `conn/secrets.rs` is the second caller — for what its
legacy-plaintext migration's save reports, now that its load reads no secret (a locked keyring,
*the* reason connections stop authenticating, used to be queued here too, and before that reached
neither a banner nor a log line; it is said per connection when one is read).
**What it queues is a `Notice`, not a sentence**, because the queue is filled in core before the
window exists and drained by an interface that says it in its own language.
`Unreadable { name, error, swept }` and `Missing { name, from }` are what `recovery_notice` and
`missing_notice` build (`notice_name` is the file name both say), `KeyringUnreadable` and
`KeyringSave` are `Hydration::notice`'s — `conn::secrets::unreadable_notice`'s in the app — and
`Sanitized::notice`'s (under `core/secrets.rs`), and
`Said` carries text its caller has already worded in the interface's language — the app's
`conn-delete-keyring-*` notice after a failed `forget_connection`. `Notice::message` is the
English, the sentences this queue used to hold, and the interface's `i18n::notice_text` is held
to it (*The string catalogue*). Like `launch::Refusal` it has no `Display`, on purpose, so a
`to_string()` that would stay English in every language does not compile.
**The drain is no longer startup-only**, and believing it was is what made the contract false for
`diagrams.json`. The app drained the queue exactly once, after the `Ui` literal, under the
comment *"Every config file has been loaded by now."* — and three loads are lazy and run long
after that line: the ER diagram's layout read and its save-side re-read, both inside drag
handlers, and the layout prune inside `delete_conn_now`'s click handler. A truncated
`diagrams.json` was therefore renamed aside and reported to nobody, the user's saved arrangements
simply gone. On the save side it is worse than silent: with the `.bak` unreadable too, the very
next drag's `save_json` writes the defaulted empty file over the recovered nothing, so the notice
has to reach the user *before* that save, which is where the call sits.
`schemaic_ui::report_recoveries(text, open)` is the one reporter — it words each drained notice
through `i18n::notice_text` and joins them with a blank line — and the startup site calls it
too rather than keeping a second spelling; it takes the two signals rather than an `OverlayUi`
because the app's own lazy load is in a closure built long before the `Ui` literal exists, and it
is cheap on an empty queue so a lazy loader can call it unconditionally. **All three lazy sites
reach it through one door now**: `schemaic_ui::load_diagram_layouts(text, open)` reads
`diagrams.json` and calls `report_recoveries` in that order, as a single act, so on the save side
the notice is queued before the caller's `save_json` can overwrite the recovered nothing. Each of
the three used to make both calls itself and carry its own copy of the comment saying why.
**That changed what can be checked, which is the part worth keeping.** The rule was
**positional** — "is this load inside `app_view`'s build?" is not a question a scan can answer —
so `app_tests::every_lazy_config_load_reports_what_it_recovered` was a *count* instead: the lazy
`diagrams.json` loads across both crates had to be outnumbered by the `report_recoveries` calls,
the extra one being the startup drain. That was weaker than it read, because the count was global
across the two files — a site that loaded and said nothing, while another site reported twice,
passed it. With one door there is nothing left to pair, and what remains is a fact a scan *can*
establish: `diagrams.json` is read in exactly one place. So that gate is gone rather than
amended, replaced by
`schemaic_ui::diagram_layout_gate::diagram_layouts_are_loaded_through_the_one_reporting_door`,
which moved crates to sit where the door is. It collects every `load_json("diagrams.json")` over
`source_gate::workspace_sources()` and asserts the site list is exactly `["lib.rs"]`, so a
failure **names the offending file** where the count could only report a number one too low —
restoring the app-side direct load failed it with `left: ["lib.rs", "schemaic-app/main.rs"]`.
**Only the counting half was rewritten, though, and "one door" does not make the reporting true
by construction**: delete `report_recoveries` from the door and the site list is still exactly
one, so the whole failure the gate exists for comes back under a green suite. Its second test,
`the_one_door_reports_before_it_returns`, reads the door's own body and requires the load, then
the report, then the return. The *order* is not tidiness — if the `.bak` was unreadable too the
caller's `save_json` writes the defaulted empty file over the recovered nothing, so the notice
has to be queued before the caller can get that far.
The lesson generalises: a rule that cannot be checked positionally can sometimes be made
*structural* instead, by reshaping the code until there is one site, and then it can.
`recovery_notice` is the corrupt-file notice — plus, for the one store whose `.corrupt` a later
save sweeps, the qualification `corrupt_sibling_is_swept` decides above; `missing_notice` is the
vanished-file one, and
it says the disappearance rather than repairing it quietly, because the sibling the file came
back from is the only copy until the next save lands and a user who does not know that has no
reason to take a backup. `load_json_strict` is the loader for *security* state rather than for
configuration: an unreadable trust store is an `Err`, never an empty one, since the default
value there is itself the insecure answer.
**`load_connections` is not a read-only operation, which is why a second process needs a second
door.** A healthy `read_bytes` sweeps the orphaned `.tmp` sibling on every read, and an
unparseable primary is renamed aside to `.corrupt` and read back off the `.bak` — both right for
the app, which owns these files and is the only thing that can repair them, and both wrong for
the headless CLI reading `connections.json` while the app runs. The `.tmp` that sweep removes is
exactly what a save in flight has just written and is about to rename into place, so a
`schemaic list` landing in that window would delete the generation the app was saving; and a
repair is not something a one-shot command was asked to perform.
`read_connections_unrecovered` parses the primary and stops — absent is the default value (what
every install looks like before its first connection is saved), unparseable is an `Err` for the
caller to report and nothing is moved — over `read_unrecovered`, the `FileStore`-taking half, so
every outcome is testable without a disk. **Only `NotFound` is an absence**: any other read
error — a denied permission, a sharing violation — is an `Err` as well, `load_json_strict`'s rule
for its reason. It was an `.ok()`, which folded all of them into "no file", so the CLI listed no
connections and called a real one unknown, sending the user after a toggle or a typo
(`an_unreadable_file_is_reported_not_read_as_absent`). The fake store keeps the two apart the
way a disk does — its `read` answers `NotFound` for a missing path and `PermissionDenied` for
anything in its `unreadable` set — because the read now tells the two apart by nothing but the
error's kind. The pin is
`the_recovering_read_sweeps_a_save_in_flight_and_the_unrecovered_one_does_not`, which runs the
two loads over the same staged `.tmp` **in one test**, because the claim is about the difference
between them and either half alone reads as a property of loading rather than a choice;
`an_unrecovered_read_leaves_every_file_where_it_found_it` is the wider half, asserted over the
fake store's op log rather than the final file set — the claim is not "it does not call `save`"
but "it renames nothing and removes nothing".
**`write_file_atomic` is the other half of this module and is for a file that is *not* ours** —
a `.sql` script the user opened, another vendor's settings file, the Antigravity claim marker.
`fs::write` truncates before it writes, so a full disk, a dropped share or a crash between the
two leaves the user's file empty with the only copy of the text in a tab about to close. It
keeps **no `.bak`** (leaving `orders.sql.bak` after every Ctrl+S is not ours to do) and it has
four load-bearing parts a tidy-up would drop. `resolved_target` follows a symlink first, because
`fs::rename` acts on the *link*: a dotfile manager's `settings.json` → a chezmoi/stow repo was
replaced by an ordinary file and the managed copy silently stopped receiving writes — a
regression the atomicity introduced, since the `fs::write` it replaced followed the link
correctly. `stage_beside` uses a **unique** staging name opened `create_new`: the fixed
`<path>.schemaic-tmp` was a pure function of the target, so two windows launching together
staged the same sibling and the loser's rename returned `NotFound` and fell through to the
truncating write this exists to avoid — and `create_new` closes the other half, since
`fs::write` is `File::create`, which follows a symlink and truncates, so another local account
pre-creating that sibling as a link redirected the victim's text at the victim's privileges. It
carries the **target's mode onto the staged file** (Unix), because the rename swaps the inode
and without it a `chmod 600 seed.sql` came back 0644. And it `sync_data`s before the rename,
with a directory sync after it where the platform allows: a rename is atomic with respect to
other *processes* and is not, on its own, ordered after the data blocks with respect to a power
loss, and this path has no `.bak` to fall back on. **What it does not pretend to keep** is a
second hard link — the new inode is a new inode, and there is no way to have both that and a
one-step replacement; atomicity wins, because the file being protected is often the only copy.
**`create_part(part, target)` is the staging rule for a data export, and it is a different
policy from the script's on purpose** (security review S31). Every export that writes a `.part`
sibling and renames it over the destination opens that sibling here — the app's result/grid
export (its `create` closure, both scopes), `cli/dump.rs`'s two writers behind `schemaic dump`
and the schema tree's folder export, and `schemaic export -o` in `cli/query.rs`. All five used
`File::create`, which is wrong twice over: it follows a symlink and truncates its target, so
another local account planting `orders.csv.part` as a link redirected the export's write at
whatever the link named, at the victim's privileges; and it creates at `0o666 & !umask`, 0644
under the usual one, so result rows and whole-database dumps landed readable by every account on
a shared host while the owner-only helpers above covered only the config directory, the log and
the AI files — nothing documented a mode for data exports at all. So it **removes whatever sits
at `part` first** — a fragment a failed export left, which a fresh export replaced anyway, or a
planted link, which `remove_file` unlinks rather than follows — then creates with `create_new`,
so a path re-planted in that window is refused rather than written through; **0600 on Unix**;
and, when `target` already exists, copies *its* permissions onto the new file, because the
rename swaps the inode and a file the user shared on purpose should stay shared — the same
reasoning `stage_beside` gives for a `chmod 600 seed.sql`, from the other direction. The
difference from `write_file_atomic` is only the *new* file: a script saved for the first time
takes the default mode, since it "narrows no permissions", where an export's new destination is
owner-only. Windows sets no mode — the file inherits the directory's ACL, as before. **No test
pins any of it**: every property is about a real file, and the workspace's no-filesystem rule
admits exactly one exception, `export::export_xlsx_chunks`. Not covered either:
`write_file_atomic`'s own fallback when the rename is refused, a direct `fs::write` of the
user-chosen target path itself, which S31 left as it was.
