# Schemaic TLS test-bed

A local CA and a deliberately-broken certificate set, so the TLS connection
modes can be developed against every outcome they have to distinguish —
including the failures a hosted endpoint can never produce on demand. A real
Neon/Supabase/RDS instance gives you exactly one case: the happy path with a
public CA. It cannot serve you an expired certificate or a hostname mismatch
because you asked it to.

Nothing here is a secret: the CA, the keys and the test users are throwaway,
generated locally, and never leave the machine.

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
| `wrongname` | plaintext | TLS | TLS | TLS | **reject: hostname** |
| `expired` | plaintext | TLS | TLS | **reject: expired** | **reject: expired** |
| `otherca` | plaintext | TLS | TLS | **reject: unknown CA** | **reject: unknown CA** |
| nothing (TLS off) | plaintext | plaintext | **reject: no TLS** | **reject: no TLS** | **reject: no TLS** |

The `wrongname` row is the one that earns the whole test-bed: it is the only
case that can tell `verify-ca` and `verify-full` apart, and a form that
collapses them looks correct until someone points it at a real endpoint.

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

On Windows against servers in WSL, point `TLS_CA` at the Windows copy
(`%USERPROFILE%\schemaic-tls\ca.crt`) — the app is a Windows process and cannot
read a WSL path. The example's header comment lists every variable.

## Sanity checks before involving the app

    openssl s_client -connect 127.0.0.1:3306 -starttls mysql    -CAfile /etc/schemaic-tls/ca.crt </dev/null 2>&1 | grep -E 'Verify return|Verification'
    openssl s_client -connect 127.0.0.1:5432 -starttls postgres -CAfile /etc/schemaic-tls/ca.crt </dev/null 2>&1 | grep -E 'Verify return|Verification'

    mariadb -h schemaic-tls.test -u <user> -p --ssl-ca=/etc/schemaic-tls/ca.crt --ssl-verify-server-cert -e "SHOW STATUS LIKE 'Ssl_cipher'"
    psql "host=schemaic-tls.test port=5432 dbname=postgres user=<role> sslmode=verify-full sslrootcert=/etc/schemaic-tls/ca.crt" -c 'select 1'

If a mode misbehaves in Schemaic, run the matching CLI first — it tells you
whether the bug is in our option mapping or in the server configuration.

## Teardown

    sudo bash setup-tls-testbed.sh --teardown

Removes the certificates, both drop-in config files, the `/etc/hosts` entry, and
restores `pg_hba.conf` from its backup. Postgres falls back to its packaged
certificate; MariaDB/MySQL goes back to plaintext. The test users are left
alone — drop them by hand if you want them gone.

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
