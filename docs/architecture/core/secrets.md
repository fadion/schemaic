# `core/secrets.rs`

`core/secrets.rs` — keeps connection secrets (DB/SSH passwords + SSH key passphrase) out of
the plaintext `connections.json`: the `SecretStore` seam, `Hydration` (the process's ledger of
what is known about the store) + pure transforms `sanitize_file` (save → move the secrets that
changed into the store, blank the disk copy; keep plaintext only if the store is unavailable)
and `forget` (delete). `RETIRED_SECRET_SUFFIXES` is the fourth transform and the
least obvious: a secret this app **stopped writing** still has to be cleaned off the machines
that stored it, so every save and every delete sweeps it. Its one member is the TLS client-key
passphrase, collected and stored by an earlier build for a feature that could never work —
`TlsPlan` carried no passphrase, so an encrypted key always failed blaming the file — and
removing the `SecretKind` arm alone would have stranded that entry in the keyring, unread and
undeletable through the app. The real keyring-backed store lives in `schemaic-conn`'s
`secrets` module (the heavy `keyring`/D-Bus dep stays out of core); pure + unit-tested via an
in-memory fake. **The connections export is a separate scrub, by design, not this one**:
`sanitize_file` keeps plaintext on disk when the store is unavailable, which is the right
answer for a save and the wrong one for a file handed to someone else, so
`conn_import::export_connections` blanks the secrets itself in a copy of the in-memory list and
never reads the file this module writes.
**Both directions of a store failure now come back as a notice, because the machine where the
keyring rule is quietly untrue is exactly the machine whose user needs to hear it.**
`SecretStore::delete` returns a `bool` — *definitely gone*, which covers a delete that succeeded
and one that found nothing — where it used to return `()` under "best effort", while its sibling
`set` already returned `bool` for the very same reason. That asymmetry was the defect:
`sanitize_file`'s own promise is that clearing a password cannot be undone by a later hydrate,
and that promise rests entirely on this call, so a delete that quietly failed left the entry in
the keyring while the disk copy said empty and the next launch hydrated the cleared password
back in. `Hydration` therefore carries `stored` (the secrets the store actually held a value for
when read, or was written by a save) and `error` (one backend message from the failed reads), and a refused delete is
reported **only** for a field this process read a value out of or wrote — on a machine with no keyring at
all, every empty field asks for a delete and every refusal is meaningless, which would be three
false alarms per connection on exactly the machines that already have a real problem.
`Hydration::notice` is the read-side notice — `persist::Notice::KeyringUnreadable`, the count
and the backend's message, its English in `Notice::message` — though since the app stopped
reading at startup no production path calls it: the app builds the same `Notice` per connection
through `conn::secrets::unreadable_notice(id)`. Both halves of what it says are load-bearing:
*unavailable this session* is why the connections stopped working, *were not deleted* is what
stops the user retyping a half-remembered password over a stored one that is perfectly intact.
`Hydration::cli_notice` is the same fact for the headless CLI's stderr — plain text, the
unreadable kinds named, and `--password-stdin` offered only when the database password is
among them, since it supplies nothing else
(`the_cli_notice_does_not_offer_stdin_for_an_ssh_secret`); `hydrate_connection` is the CLI's
load, one connection and none of the kinds its caller `supplied`
(`hydrating_one_connection_skips_what_the_caller_supplied`) — **nor any `unused_kinds`**, the
secrets the connection has no use for as configured: both SSH kinds with no tunnel, the
passphrase under password auth, the password under a key pair, both under the agent
(`the_unused_kinds_follow_the_ssh_auth_method`). Asking for those on a locked keyring warned
about two SSH secrets on a connection with no tunnel, telling the user to unlock it and retry a
command that had just succeeded (`a_connection_with_no_tunnel_is_not_asked_for_ssh_secrets`).
The app's connection form still reads every kind, because it can switch the auth method and
should find the secret there.
**The app reads a connection's secrets the first time something needs them, not at startup,
and that gave an empty field a third meaning.** After an update macOS asked for the login
password once per stored secret: the keychain ties *Always allow* to the code signature, the
macOS build is unsigned (`app/update.md`), and the eager `hydrate_file` read every kind of every
connection at launch, so every prompt arrived at once, for connections the user might not open
that day. Reading lazily does not fix the signature — only a stable signing identity would, and
one keychain item holding every secret was rejected to keep the layout one entry per connection
per kind on every platform — it moves each prompt to the moment its connection is used, and
reads each secret once per process. What it cost was the save path's footing. An empty field
used to mean *read and absent* or *cleared by the user*, and `sanitize_file` deleted on both;
now every connection not yet used has empty fields too, and the first ordinary save — a
read-only toggle rewrites the whole file — would delete their credentials. So `Hydration` is
the process-wide ledger of what is known about the keyring, not what one load learned. Its
`read` is the secrets whose field in the list being saved **carries the store's answer** — read
into it, written from it by a save, or cleared in it after one of those — and they are the only
empty fields a save deletes for. A field never read is left exactly as it is in the store, which
replaced the `is_unreadable` check, a failed read being one more way of never having been read
(`a_save_leaves_alone_every_secret_it_never_read`, red against the old `sanitize_file`). The
private `held` map is what the store is known to hold — `Some` a value, `None` no entry — and
answers two questions: a second read of a secret is answered from it (`known`), so one *Always
allow* lasts the process; and a non-empty value it already holds is not written again, only
blanked in the disk copy, since on macOS a write to an item another signature created prompts as
a read does, and rewriting every secret on every save would turn one prompt per item after an
update into one per item per save (`a_save_does_not_rewrite_a_secret_the_store_already_holds`,
red against the old code too). Its values are secrets, so `Hydration`'s `Debug` is hand-written
and prints only the keys (`a_hydration_debug_print_names_no_secret`).
**A read and its delivery are two steps, and the gap between them is load-bearing.** `record`
puts the store's answer in `held` and does **not** mark the secret read: the answer is in the
cache, not in the list a save writes, and a save landing between the two must still see a
never-read field. `deliver(conn, kinds)` is the other half — it fills `conn`'s empty fields from
`held`, keeps a non-empty one (what the user typed, or plaintext the file still carries), and
marks each it had an answer for as read; an unknown or unreadable one stays empty and unmarked
(`a_read_counts_for_the_save_only_once_it_is_delivered`, `an_unreadable_secret_is_not_delivered`).
It has to run on the thread that owns the list, in the step that writes to it, so no save can fall
between the field being filled and the secret being marked. `absorb_stored(saved, file)` now
takes the saved list as well as the `Sanitized`: a secret the save wrote becomes read and held at
the list's value, and one it deleted — `Sanitized::cleared`, the deletes that succeeded — is held
as `None`, or typing the same value back in after a clear would be skipped as unchanged
(`a_changed_or_retyped_secret_is_written`). `forget` drops `read` and `held` with the rest, so a
reused id reads afresh. `plaintext_on_disk(file)` is the one question the app's load still asks,
of the file and not the store; `tunnel_kinds(conn)`, `unused_kinds`' complement among the SSH
kinds, is what the app reads before it dials a bastion. `hydrate_file` survives only under
`#[cfg(test)]`, the tests' model of a load that read everything; `hydrate_connection` behaves as
before and goes through `record` + `deliver` in one step, the CLI owning its connection outright.
`Sanitized { file, in_the_clear, undeleted }` is the write-side counterpart — the sanitized file
plus the two facts the save established, which used to be discarded at the only call site — and
its `notice()`, `Notice::KeyringSave` with the two counts, names the consequence rather than the
mechanism: *your passwords are in a file
you are about to send someone* (the folder Settings → General's **Open folder** button opens,
beside the log), and *the next launch will fill this cleared field back in*. `forget` returns
whether a deleted connection's entries are all definitely gone, which matters because ids are
reused.
**`conn/secrets.rs`'s two classifications are free functions so they can be tested**, which is
the whole reason they were pulled out of the trait impl. `classify_get` decides that `NoEntry`
alone means *there is no secret here* and everything else — a locked keyring, a denied prompt, a
platform failure — means *we could not read it*; `classify_delete` is the same rule the other
way. Core pins both consequences, but only ever against a fake that reports errors correctly by
construction, so collapsing the match at the real backend left every one of those tests green
while credentials were destroyed — the seam at a trait impl rather than at a function call, and
that file was at zero coverage. On the store side `load_connections` reads no secret and queues
on `persist::queue_notice` (the startup channel the config-recovery modal drains) only what its
legacy-plaintext migration's save reports; a read that fails is told per connection instead
(`conn/secrets.rs`); `save_connections` returns `Option<persist::Notice>` and `main.rs`'s `persist_conns` shows it
in the error modal through `i18n::notice_text` — once per session per distinct notice,
`told_once` remembering the `Notice` value itself (it derives `Hash`), because the alternative is a modal on every
read-only toggle and every connection switch for as long as the keyring is down. And the
process-level `last_hydration` is read with `into_inner` rather than `unwrap_or_default`: a
poisoned mutex used to hand the save a *default* `Hydration` with nothing marked unreadable, so
the save read every empty field as a password the user had cleared and deleted it. The failure
direction of a lock has to be *keep what we knew*, not *know nothing*.
