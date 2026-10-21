# `core/secrets.rs`

`core/secrets.rs` — keeps connection secrets (DB/SSH passwords + SSH key passphrase) out of
the plaintext `connections.json`: the `SecretStore` seam + pure transforms `hydrate_file` (load →
fill empty fields from the store, flag legacy plaintext for migration), `sanitize_file` (save →
move secrets into the store, blank the disk copy; keep plaintext only if the store is
unavailable) and `forget` (delete). `RETIRED_SECRET_SUFFIXES` is the fourth transform and the
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
`conn_import::export_connections` blanks the secrets itself in a copy of the hydrated list and
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
at load) and `error` (one backend message from the failed reads), and a refused delete is
reported **only** for a field this load read a value out of — on a machine with no keyring at
all, every empty field asks for a delete and every refusal is meaningless, which would be three
false alarms per connection on exactly the machines that already have a real problem.
`Hydration::notice` is the read-side notice — `persist::Notice::KeyringUnreadable`, the count
and the backend's message, its English in `Notice::message` — and both halves of what it says
are load-bearing:
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
The app's whole-file `hydrate_file` still reads every kind, because a form can switch the auth
method and should find the secret there.
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
that file was at zero coverage. On the store side `load_connections` puts `Hydration::notice` on
`persist::queue_notice` (the startup channel the config-recovery modal drains),
`save_connections` returns `Option<persist::Notice>` and `main.rs`'s `persist_conns` shows it
in the error modal through `i18n::notice_text` — once per session per distinct notice,
`told_once` remembering the `Notice` value itself (it derives `Hash`), because the alternative is a modal on every
read-only toggle and every connection switch for as long as the keyring is down. And the
process-level `last_hydration` is read with `into_inner` rather than `unwrap_or_default`: a
poisoned mutex used to hand the save a *default* `Hydration` with nothing marked unreadable, so
the save read every empty field as a password the user had cleared and deleted it. The failure
direction of a lock has to be *keep what we knew*, not *know nothing*.
