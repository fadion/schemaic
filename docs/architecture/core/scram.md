# `core/scram.rs`

`scram.rs` — PostgreSQL's **SCRAM-SHA-256 password verifier**, computed here so a role's
password never reaches the server as text. `ALTER ROLE "x" PASSWORD 'hunter2'` is logged
verbatim under `log_statement = 'ddl'`, pgaudit or any `log_min_duration_statement` the
statement exceeds, and sits in `pg_stat_activity.query` while it runs — so the account form's
create and reset were writing the password into the server's own logs. PostgreSQL 10 and later
store a password that is *already* a verifier exactly as given (what `psql`'s `\password` relies
on), so `PASSWORD 'SCRAM-SHA-256$4096:…'` sets the same password with nothing recoverable in the
statement. `verifier(password, &Salt)` builds the server's own format,
`SCRAM-SHA-256$<iterations>:<salt>$<StoredKey>:<ServerKey>` in standard base64, at `ITERATIONS`
(4096, PostgreSQL's default `scram_iterations`) over a 16-byte `Salt`, through a hand-written
PBKDF2-HMAC-SHA-256 of **one block** — SCRAM's output is exactly one HMAC wide.
`verifier_with(password, &Salt, iterations)` is the same at a count the caller names, and
`verifier` is it at `ITERATIONS`: a server stores a verifier as given *whatever* its own
`password_encryption` and `scram_iterations` say, so one built at the default under a hardened
setting silently weakens every password it sets — `users::PasswordPolicy` is what reads the
server's count and decides whether a SCRAM verifier is sent at all. `0` is not an iteration
count and answers `None`. The `sha2`,
`hmac` and `base64` it calls are the versions `postgres-protocol` already put in the lock, so
making them direct dependencies of core added no new version.
**A password is hashed exactly when SASLprep would leave it unchanged; one it would rewrite,
and an empty one, is `None`.** The client SASLpreps (RFC 4013) the password it types at login
before deriving anything, and the server does the same to one handed to it as text.
`saslprep_fixed` asks only whether the two steps that *rewrite* — §2.1's mapping (a non-ASCII
space to U+0020, the commonly-mapped-to-nothing characters removed) and NFKC — are the
identity, with pure ASCII answering `true` at once, the fast path libpq and the server take.
That is the whole question because every later step only *checks*: a prohibited, bidi or
unassigned character fails SASLprep, and on failure libpq, `postgres-protocol` and the
server's `pg_saslprep` all fall back to the raw bytes. So for a fixed password every path
hashes the same bytes, and no implementation's opinion of which characters are prohibited
matters — `tab\there` and `x\u{0378}` (unassigned) are hashed. Before this the rule was
printable ASCII only, which sent `pässword` or `日本` to the logs as text for want of a
SASLprep. **A password SASLprep would rewrite — `ｆｕｌｌ`, a ligature, a decomposed `ä`, a
no-break space, a soft hyphen — is still declined deliberately**, not for want of the code:
hashing the rewritten form would rest on this crate's Unicode tables agreeing with every
client's, and a disagreement is a lockout with a successful statement. The caller sends those
as typed, so such a password still reaches the server as text. `stringprep` and
`unicode-normalization` are the crates and versions `postgres-protocol` already brings, so the
lock gained only the direct edges. `a_password_saslprep_leaves_alone_is_hashed` and
`a_password_saslprep_would_rewrite_is_not_hashed` pin the two sides, and
`fixed_agrees_with_the_clients_saslprep` holds `saslprep_fixed` to `stringprep::saslprep`
wherever the latter succeeds. Checked live on PostgreSQL 16 by the change's author: roles set
from these verifiers for `pässword`, `日本`, `tab\there`, `Ελληνικά` and `x\u{0378}` logged in
through both tokio-postgres and libpq (`psql`), and a wrong password was refused.
**The salt is always an argument, never drawn here**: the form stamps it into
the draft once (`users::AccountDraft::scram_salt`), so the preview and the Apply run one
identical statement and `ChangeSet::emit` stays a pure function. Two of the tests are oracles
rather than round trips, because a verifier computed wrong is the silent failure — the server
stores it verbatim, the statement succeeds, and the account takes no password anyone holds.
`matches_a_verifier_postgresql_built` reproduces byte-for-byte a verifier PostgreSQL 16.14 itself
built for `correct horse ~ battery!`, read back from `pg_authid` under the same salt — the space,
`~` and `!` sit on the edges of printable ASCII — and `logs_in_to_the_rfc_7677_exchange`
validates RFC 7677 §3's exchange from the verifier's own `StoredKey`/`ServerKey`, which is what
a server does at login. The live tier's `a_salted_password_logs_in_on_create_and_on_reset` is
the third check, against a real server.
