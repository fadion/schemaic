# Schemaic TLS test-bed

A local CA and a deliberately-broken certificate set, so the TLS connection
modes can be developed against every outcome they have to distinguish —
including the failures a hosted endpoint can never produce on demand. A real
Neon/Supabase/RDS instance gives you exactly one case: the happy path with a
public CA. It cannot serve you an expired certificate or a hostname mismatch
because you asked it to.

Nothing here is a secret: the CA, the keys and the test accounts are throwaway,
generated locally, and never leave the machine. **They are still credentials.**
The password below is printed in this file, so the accounts are created for
`localhost`/`127.0.0.1` only and granted rights on one sandbox database — not,
as they once were, at host `%` with `ALL PRIVILEGES ON *.*` on a server whose
`bind_address` is `0.0.0.0`. `client.key` is `0600` like every other key here;
the copies made for a Windows app are chowned to you. Teardown drops the
accounts and removes the copies.

## Install

    sudo bash setup-tls-testbed.sh              # add --pg-clientcert for the mutual-TLS path

It detects the PostgreSQL cluster and the MariaDB/MySQL drop-in directory, and
skips whichever engine is absent — one is enough to work on the connection form.
Every detection has an override; see the header of the script.

To inspect what it would build without touching a server (no root needed):

    SCHEMAIC_TLS_DIR=/tmp/tls bash setup-tls-testbed.sh --certs-only

If the app runs on Windows against servers in WSL, add this to
`C:\Windows\System32\drivers\etc\hosts` as Administrator:

    127.0.0.1 schemaic-tls.test

`verify-full` verifies a *name*, and that is the name in the certificates. WSL2
forwards localhost, so the Windows side reaches the WSL servers through it. The
client-side files (`ca.crt`, `otherca.crt`, `client.crt`, `client.key`) are
copied into a `schemaic-tls` folder in the Windows user profile for the
connection form to point at.

## What it builds

Under `/etc/schemaic-tls` (or `$SCHEMAIC_TLS_DIR`):

| Certificate | Signed by | Names | Purpose |
|---|---|---|---|
| `server-good` | Test CA | `localhost`, `schemaic-tls.test`, `127.0.0.1` | every mode accepts |
| `server-wrongname` | Test CA | `wrong.example` | `verify-ca` accepts, `verify-full` rejects |
| `server-expired` | Test CA | correct | expiry rejection (dated 2 → 1 years ago) |
| `server-otherca` | Other CA | correct | unknown-CA rejection |
| `client` | Test CA | CN `schemaic_client` | mutual TLS |
| `ca.crt` / `otherca.crt` | — | — | the trust files the connection form points at |

Users created: MariaDB/MySQL `schemaic_ssl` (`REQUIRE SSL`) and `schemaic_x509`
(`REQUIRE X509`), password `schemaic` unless `SCHEMAIC_TLS_PASSWORD` says
otherwise; with `--pg-clientcert`, the Postgres role `schemaic_client`.

## The matrix

One server presents one certificate, so the negative cases come from swapping
underneath a configured connection:

    sudo bash swap-server-cert.sh wrongname both
    bash swap-server-cert.sh status

| Server presents | disable | prefer | require | verify-ca | verify-full |
|---|---|---|---|---|---|
| `good` | plaintext | TLS | TLS | TLS | TLS |
| `wrongname` | plaintext | TLS | TLS | TLS (PG) / **reject: hostname** (MySQL) | **reject: hostname** |
| `expired` | plaintext | TLS | TLS | **reject: expired** | **reject: expired** |
| `otherca` | plaintext | TLS | TLS | **reject: unknown CA** | **reject: unknown CA** |
| nothing (TLS off) | plaintext | plaintext | **reject: no TLS** | **reject: no TLS** | **reject: no TLS** |

The `wrongname` row is the one that earns the whole test-bed: it is the only
case that can tell `verify-ca` and `verify-full` apart, and a form that
collapses them looks correct until someone points it at a real endpoint.

**And on MySQL/MariaDB it is currently the same cell in both columns**, which
is exactly what running this row found. `mysql_async` 0.37 implements its
"skip domain validation" toggle by matching `"NotValidForName"` in the
verifier's error text, and rustls 0.23 raises `NotValidForNameContext`, whose
`Display` has no such substring — so the arm never fires and `verify-ca` also
rejects a name mismatch there. Measured twice against the same server, same CA,
same binary, differing only in the name dialled. The form says so
(`SslMode::caveat`), and `db::tls`'s
`the_driver_still_reads_the_verifier_error_by_its_words` turns red the day the
drivers agree again — at which point this paragraph and that column come back
out. PostgreSQL is unaffected: Schemaic's own verifier there names both
spellings.

The last row is what users hit on a misconfigured RDS. Produce it by deleting
the `99-schemaic-tls.cnf` drop-in and restarting, or by pointing the connection
at a port with no TLS at all.

Two more cases the table does not cover:

- **Server refuses plaintext.** Uncomment `require_secure_transport = ON` in the
  MariaDB/MySQL drop-in, restart, and connect with `disable` — that is the
  Azure/RDS "SSL connection is required" error.
- **Per-user TLS enforcement.** Connect as `schemaic_ssl` with `disable`; the
  server rejects the *login* rather than the transport, which is a different
  error path and deserves its own message.

## Walking the matrix without the app

`cargo run -p schemaic-db --example tls_matrix` drives every mode through the
real driver stack and prints the two columns that matter — trusting the CA that
signed the server, and trusting one that did not. It takes the same certificates
this script generates:

```bash
TLS_CA=/etc/schemaic-tls/ca.crt cargo run -p schemaic-db --example tls_matrix
```

PowerShell has no inline `VAR=value` prefix, so set them first:

```powershell
$env:TLS_CA = "$env:USERPROFILE\schemaic-tls\ca.crt"; cargo run -p schemaic-db --example tls_matrix
```

On Windows against servers in WSL, `TLS_CA` must be the **Windows** copy the
setup script placed in your profile — the app is a Windows process and cannot
read a WSL path.

Set either CA variable to `os` to use the operating system's trust store instead
of a file. It is a word rather than an empty value because PowerShell *deletes*
a variable assigned `''`, so the empty spelling would silently fall back to the
default path. The example's header comment lists every variable.

## Sanity checks before involving the app

    openssl s_client -connect 127.0.0.1:3306 -starttls mysql    -CAfile /etc/schemaic-tls/ca.crt </dev/null 2>&1 | grep -E 'Verify return|Verification'
    openssl s_client -connect 127.0.0.1:5432 -starttls postgres -CAfile /etc/schemaic-tls/ca.crt </dev/null 2>&1 | grep -E 'Verify return|Verification'

    mariadb -h schemaic-tls.test -u <user> -p --ssl-ca=/etc/schemaic-tls/ca.crt --ssl-verify-server-cert -e "SHOW STATUS LIKE 'Ssl_cipher'"
    psql "host=schemaic-tls.test port=5432 dbname=postgres user=<role> sslmode=verify-full sslrootcert=/etc/schemaic-tls/ca.crt" -c 'select 1'

If a mode misbehaves in Schemaic, run the matching CLI first — it tells you
whether the bug is in our option mapping or in the server configuration.

## Teardown

    sudo bash setup-tls-testbed.sh --teardown

Removes, in full:

- the certificate directory (`/etc/schemaic-tls` by default);
- both drop-in config files, so Postgres falls back to its packaged certificate
  and MariaDB/MySQL goes back to plaintext;
- `/etc/mysql/ssl`, **restored from the snapshot setup takes** if that directory
  already held anything — setup writes over `{ca,server}.{crt,key}` there;
- `pg_hba.conf`, restored from its backup;
- the `/etc/hosts` entry;
- the MariaDB/MySQL accounts `schemaic_ssl` and `schemaic_x509`, and their
  sandbox database — they authenticate with a password this file prints, so
  leaving them behind is not a tidiness question;
- the client copies written into the Windows profile, `client.key` included.

PostgreSQL's test roles are left alone — drop them by hand if you want them
gone. (`cert` auth matches a role name against the certificate CN, so they carry
no password.)

## What this cannot cover

Worth one manual smoke test against a free hosted instance before shipping,
because none of it reproduces locally:

- a public CA chain resolved through the OS/webpki root store, which is a
  different code path from loading a CA out of a file;
- SNI-based routing (Neon, PlanetScale) — a driver that omits SNI fails there
  and nowhere else;
- TLS 1.2-only or cipher-restricted servers (older hosted MySQL);
- rustls being stricter than OpenSSL about SAN-less certificates and unusual
  intermediate chains, which is the likeliest "works locally, fails on RDS"
  class.
