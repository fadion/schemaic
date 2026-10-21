# `db/tls.rs`

**`tls.rs` translates a connection's TLS settings into the networked drivers**, and only
translates: the decisions were already made by `core::connection::Tls::plan`, which collapses the
five libpq `sslmode` levels into the four booleans a handshake is actually made of. The drivers
spell those very differently — `mysql_async` takes two `danger_*` toggles on an `SslOpts`, while
`tokio_postgres` takes an `SslMode` for the negotiation and leaves every verification question to
the rustls `ClientConfig` behind it — and deriving each from the five modes separately is how
`verify-ca` comes to mean one thing on MySQL and another on PostgreSQL. Three things here are
load-bearing. **The verifier ladder**: rustls checks the chain and the host name in one call, so
`verify-ca` cannot be *configured*, only wrapped — `NameAgnosticVerifier` is the real webpki
verifier forgiving exactly `CertificateError::NotValidForName`, because a wrapper that accepted
everything on any error would turn `verify-ca` into `require` wearing a stronger label, silently
admitting expired and unknown-CA certificates. **The tunnel's name**: `Db::connect` rewrites the
endpoint to `127.0.0.1:<local port>`, which would have `verify-full` compare a perfectly good
certificate against the loopback address, so the name to check is carried over in the same step
that moves the address (`TlsPlan::hostname_override` → `FixedNameVerifier`, and MySQL's
`with_danger_tls_hostname_override`) — substituting the name rather than skipping the check,
which would make `verify-full` mean `verify-ca` for every tunnelled connection. **The `prefer`
retry**: a server without TLS fails the handshake rather than declining it, so "encrypt if you
can" exists only as a second attempt in `Db::open`, and offering that second attempt to anything
above `Prefer` would let the strongest half of the setting report success in plaintext. **The
retry inspects the error**, and that is not a detail: its only condition used to be
`fallback_to_plaintext`, so `prefer` retried after a wrong password (twelve connect attempts for
ten pings, measured) and after anything an attacker can provoke mid-handshake — one injected RST
and the whole operation was in cleartext. `should_retry_plaintext` matches the one variant the
mode exists for, `NoClientSslFlagFromServer`.
PostgreSQL needs no such retry — its driver negotiates the downgrade itself — which is why the
mapping is `pg_ssl_mode`, not a shared retry. Cancellation goes through `pg::cancel_query` rather
than a bare `NoTls`: cancelling opens a *second* connection, and a plaintext one is refused by
exactly the servers this setting exists for, with the failure discarded. That is a property of a
**set of call sites**, not of the function — the function was right the whole time two of the ten
sites bypassed it — so the gate is `session::pg_cancel_gate`, which scans the crate for a
`cancel_query` outside the module that defines the helper — and outside `mssql.rs`, which it skips
by name, because `tiberius::Client::cancel_query` shares only the name: it is a TDS attention on
the query's *own* connection, so there is no second connection and no transport to choose.
**Its needle stops before the paren**,
because `tokio_postgres::CancelToken` has two of them: `cancel_query(tls)` and
`cancel_query_raw(stream, tls)`, the second being what a cancel riding an already-open socket
would reach for — so a needle ending in `(` sees the first and walks past the second, which
commits the identical failure. `CancelToken::cancel_query` is listed beside it for the UFCS
spelling, and both are assembled from pieces so the gate does not find itself: it lives in a file
it reads. It also asserts it scanned at least five files, this being the one gate of the family
that reads a *directory* rather than a named source, so a moved or renamed `src` would otherwise
make it pass by reading nothing. Both cancels are bounded by
`CANCEL_TIMEOUT`: best-effort and unbounded are not the same word, and a Stop against a host that
has gone away hangs inside a modal whose every exit maps to that same Stop.
**Two things the ladder cannot deliver, said out loud rather than left to a connect.**
`verify-ca` on MySQL/MariaDB also rejects a host-name mismatch: `mysql_async` 0.37 implements its
skip-domain-validation toggle by matching `"NotValidForName"` in the verifier's error *text*, and
rustls 0.23 raises `NotValidForNameContext`, whose `Display` has no such substring — so the two
verifying rungs are one mode there. `SslMode::caveat` puts that under the picker on those two
engines alone (`verify_ca_checks_host_name`; SQL Server's own rustls configuration does skip
the name), and
`the_driver_still_reads_the_verifier_error_by_its_words` pins what is true **now**, so it turns
red the day the drivers agree again. And a **passphrase-protected client key** is refused by name
(`parse_key`), with the `openssl pkcs8` command to fix it: the form used to collect a passphrase
for one and hand it to nobody, so an encrypted key failed with "is not a PEM private key",
blaming a file that was perfectly good. **There are two spellings of "encrypted" and only PKCS#8's
was recognised**, which is why that test is `is_encrypted_key` rather than a `contains` at the
call site. PKCS#8 says it in the header (`-----BEGIN ENCRYPTED PRIVATE KEY-----`); the legacy
RFC 1421 form keeps an ordinary `EC PRIVATE KEY` / `RSA PRIVATE KEY` header and puts the fact in a
`Proc-Type: 4,ENCRYPTED` line above the base64, with no occurrence of the string being matched —
and that is what the *plain* `openssl ec -aes256` and `openssl rsa -aes256 -traditional` produce.
Unrecognised, those reached `from_pem_slice`, which cannot base64-decode a header line and answers
`Base64Decode(InvalidCharacter(45))`, reported as the very message this arm exists to stop
emitting. It keys on `Proc-Type` and not on the header, because the header is shared with the
*unencrypted* legacy key that must still be read
(`an_unencrypted_legacy_key_is_not_mistaken_for_an_encrypted_one`, beside
`a_legacy_encrypted_key_is_named_too`), and
on `Proc-Type` rather than `DEK-Info`, because that is the field which says encrypted —
`DEK-Info` only names the cipher. `read_certs` refuses a file with **no PEM section** for
the same reason — a DER `.crt` is what Windows' *Export certificate* writes, it came back as
`Ok(vec![])`, and `preflight` passed it while PostgreSQL rejected the identity and MySQL
presented it.
**Every failure this module composes is a `core::model::TlsFailure`** inside
`DbError::ConnectFailed`, built by the private `tls_failed` — the set-up and trust-store
failures, a rejected client certificate, a name no certificate can carry, the CA, certificate
and key file refusals above — so the window words them and rustls's or the file system's error
inside passes through. They were `DbError::Connect` strings, the `openssl` commands included,
and reached the window in English; their tests now match `ConnectFailed` and read the same
`Display`.
**How any of this is verified, since none of it can be tested purely:** the unit tests here cover
the decisions and nothing pure covers a handshake. `db/examples/tls_matrix.rs` walks every mode
against a live server and prints what each one did, and `scripts/tls-testbed/` is what it is
pointed at — a local CA and a deliberately-broken certificate set (`wrongname`, `expired`,
`otherca`), because a real hosted endpoint gives you exactly one case, the happy path, and cannot
serve an expired certificate because you asked. `swap-server-cert.sh` moves the server from one
to the next; the README carries the matrix the harness reproduces.
**The harness reports the transport, not just the connect**, and that is the whole of its
usefulness: a `ping()` says only "the server let me in", and printing that for a plaintext
connection and a TLS one alike would have reported `prefer` falling back after a wrong password
as a pass — so every successful cell asks the server (`Ssl_cipher`, `pg_stat_ssl`). Its negative
control (`TLS_WRONG_CA`) has no derived default for the same reason: rewriting `ca.crt` in
`TLS_CA` is a no-op for any other file name, so column B silently became column A while the
matrix read clean. The test-bed's accounts are `localhost`-only, scoped to one sandbox database,
and dropped by teardown: the README prints their password, and it is still a credential.
**Trust anchors are the OS store** (`default_roots`, `rustls-native-certs`), read **once** —
one-connection-per-operation puts this on the path of every query and health poll, and
enumerating the Windows store per connection would be a cost paid thousands of times for an
answer that cannot change mid-session. `webpki-roots` remains only as a fallback for a machine
with nothing readable, and as a *fallback* rather than a union on purpose: reading the OS store
is worth doing because an administrator's decisions there bind, so a CA they removed has to stop
being trusted. **The workspace's `webpki-roots` must stay on `mysql_async`'s major**, and that is
a real constraint rather than tidiness: the bundled arm is the one case where the driver's own
built-in roots are deliberately left switched *on*, because they **are** our set — which was
false while the workspace held 0.26 and the driver 1.0, so the two engines verified against two
Mozilla snapshots under a promise of one. Nothing in the crate can see that; the guard is the
dependency. Keeping the two engines on one set is the part that takes work — `mysql_async`
adds its own compiled-in roots to whatever it is given, so `mysql_ssl_opts` sets
`with_disable_built_in_roots` and passes the anchors as raw DER buffers, which its loader
accepts only because DER trips no PEM section (pinned by a test, since it is an assumption about
someone else's parser). That switch is deliberately **not** made for `prefer`/`require`: the
driver builds a `WebPkiServerVerifier` even when it will never consult it, and that builder
fails on an empty root store, so disabling the built-ins for a mode that names no CA would break
the default mode of every connection. Two things to know before blaming a server. The anchor
count is legitimately smaller than the certificate manager's — `rustls-native-certs` returns only
server-auth roots, and a Windows box measured here held 69 and yielded 37, the rest being
code-signing and timestamping CAs, with `ISRG Root X1` among those kept — so a low number is not
itself a symptom. Separately, Windows populates its root program lazily, so a certificate every
browser accepts can still fail `verify-ca` with `UnknownIssuer`, and one installed during a
session needs a restart to be seen.
**SQL Server is the third driver, and it takes PostgreSQL's configuration rather than a third
spelling.** Upstream `tiberius` builds its rustls config from its own trust settings and offers no
way to supply one, so the vendored copy grows `Config::rustls_client_config` (`PATCHES.md`, patch
2) and `mssql_client_config` hands it the same `client_config` the verifying modes build for
PostgreSQL — so SQL Server's `verify-ca` is this module's, not the driver's. The TDS half
is `mssql_encryption`: `prefer` is `On`, `require` and both verifying modes are `Required`,
refusing a server that cannot — `pg_ssl_mode`'s line — and **`disable` is `Off`, not
`NotSupported`**, because TDS's `Off` still encrypts the *login* packet, the one carrying the
password, where `NotSupported` would send it as written; every Microsoft driver does the same
with `Encrypt=false` (`a_sql_server_connection_never_sends_its_password_in_the_clear`). **The
modes that trust no certificate do not even parse one**, and that is load-bearing: a SQL Server
with no certificate configured generates a self-signed one at start that rustls cannot read —
every handshake failed with `UnsupportedCertVersion`, `disable` included since the login is
encrypted regardless, measured on SQL Server 2022 CU27 in Docker. `NoVerification` still checks
the handshake signature against the certificate's key, which needs the parse, so those modes get
`AnyCertificate`, which skips it. That costs nothing an attacker could use: where no certificate
is trusted, an attacker presents their own and signs with it. The verifying modes keep the full
check and so need a real certificate, which the auto-generated one never is. **That argument is
about the check, not about what the session is trusted to carry**, and for a while it was read
as the whole of it: an Entra token — a bearer token for every Azure SQL database the account can
reach, not a secret for this one server — went over `Off` + `AnyCertificate` whenever a
connection sat at the default `Disable`. That sign-in is raised to `VerifyFull` before a plan
reaches this module (`AuthMode::tls_floor`, under `connection.rs`), so nothing here changed: a
mode that trusts no certificate is still right for what the user chose to send over it.
