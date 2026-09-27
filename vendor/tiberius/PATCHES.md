# tiberius 0.13.0, vendored

This is the `tiberius` crate as published to crates.io at 0.13.0, used through
`[patch.crates-io]` in the workspace `Cargo.toml`. It is the SQL Server (TDS)
driver behind `schemaic_db::mssql`. Everything under `src/` is upstream's, byte
for byte, except the changes listed below.

**Left out of the copy:** `tests/`, `examples/`, `docs/`, `docker/`,
`CHANGELOG.md`, `Cargo.toml.orig`, the Nix and compose files, and the published
`Cargo.lock`. The build reads none of them. `Cargo.toml` is the normalized
manifest crates.io serves, with the changes below.

**Added for vendoring, not changes to tiberius:**

- this file;
- `rustfmt.toml`, which keeps `cargo fmt --all` (it formats local path
  dependencies too) from reformatting upstream's code;
- in `Cargo.toml`: the `[[example]]` and `[[test]]` targets and every
  `dev-dependencies` entry are removed, because the files they name were not
  copied. Upstream's `[lints]` is replaced with one allowing every lint, as for
  `vendor/floem`, because Cargo caps a registry crate's warnings but not a path
  dependency's.

To take a new tiberius release: vendor it the same way, re-apply every entry
below, and drop any that upstream has absorbed. When the list is empty, delete
this directory and the `[patch]` entry.

## Changes against upstream

Each change is marked with a `schemaic patch (PATCHES.md)` comment, so
`grep -rn "schemaic patch" src Cargo.toml` finds all of them.

### 1. ring instead of aws-lc-rs

**Why.** The workspace keeps exactly one rustls crypto provider, `ring` (see
the `rustls` entry in the workspace `Cargo.toml`). Upstream's rustls backend
names `rustls::crypto::aws_lc_rs` in its source, and takes `tokio-rustls` with
default features, which switch aws-lc-rs on. That means a C/cmake/NASM build,
and a second provider next to `ring`, which makes rustls unable to pick one from
crate features.

**What.**

- `Cargo.toml`: `tokio-rustls` is taken with `default-features = false` and
  `features = ["ring", "tls12", "logging"]`.
- `src/client/tls_stream/rustls_tls_stream.rs`: the `aws_lc_rs` import, the
  `resolve_crypto_provider` fallback, and the two tests that built an
  aws-lc-rs provider now use `ring`. The tests are not built here, but are kept
  in step so re-vendoring can diff cleanly.

### 2. `Config::rustls_client_config`: a caller-supplied rustls configuration

**Why.** `schemaic_db::tls` turns a connection's TLS settings into one rustls
`ClientConfig` for every networked engine. That is what keeps `verify-ca` and
`verify-full` meaning the same thing on MySQL, PostgreSQL and SQL Server.
Upstream builds its own configuration from its own trust settings and offers
no way to supply one.

**What.**

- `src/client/config.rs`: a `rustls_client_config:
  Option<Arc<rustls::ClientConfig>>` field on `Config` (rustls feature only),
  `None` by default, and a `pub fn rustls_client_config(&mut self, …)` setter.
- `src/client/tls_stream/rustls_tls_stream.rs`, `TlsStream::new`: when the
  field is set, a copy of that configuration is used for the handshake. The only
  change made to the copy is adding the TDS 8.0 ALPN protocol under
  `EncryptionLevel::Strict`, exactly as upstream's own path does. Upstream's
  trust, bypass and client-certificate settings are then not consulted.

### 3. `QueryStream::rows_affected`: the row counts a batch reports

**Why.** The editor reports how many rows an `UPDATE` or `DELETE` touched,
and it runs the user's text through `simple_query` because it cannot know in
advance whether that text returns rows. Upstream's `QueryStream` skips every
`DONE` token, and the count is only in those. `Client::execute` keeps them, but
it discards any rows, so a caller would have to guess which of the two to use
before sending the statement.

**What.**

- `src/tds/stream/query.rs`: a `rows_affected: Vec<u64>` field on
  `QueryStream`. `poll_next` pushes the count of every `DONE`, `DONEINPROC` and
  `DONEPROC` token that carries one (`DONE_COUNT`) instead of skipping it, and
  a `pub fn rows_affected(&self) -> &[u64]` returns them. `forward_to_metadata`
  does the same for the tokens it skips: `simple_query` calls it before handing
  the stream back, and for a statement with no result set that is every token
  the statement produces, so without it the counts were all gone before the
  caller saw the stream.
- `src/tds/codec/token/token_done.rs`: `pub(crate) fn has_count()`, the
  `DONE_COUNT` test `rows()` already makes.

### 4. `money` decodes as an exact `Numeric`, not an `f64`

**Why.** `money` is a scaled 64-bit integer on the wire (the value times
10,000), and runs to 922,337,203,685,477.5807. An `f64` holds integers exactly
only to 2^53, so upstream's `ColumnData::F64` lost cents on any amount past
about 900 billion, and printed `12.5` for `12.5000` below it. The grid shows
the value as text, and a wrong digit there is a wrong figure in a report.

**What.** `src/tds/codec/column_data/money.rs`, `decode`: `money` and
`smallmoney` come back as `ColumnData::Numeric` with scale 4, built from the
scaled integer. A `NULL` is `Numeric(None)`. `FromSql` for `f64` no longer
reads a `money` column as a result; nothing here uses it. The module's own
tests for the two decode arms still expect `F64` and are not built here.

### 5. `cancel_query` reads past the aborted request's reply to the acknowledgement

**Why.** After an Attention, SQL Server finishes its reply to the request it
aborted — ending that message — and can send the acknowledging `DONE_ATTN` as a
message of its own. Upstream stops at the first end-of-message and answers
`Protocol("Never got a DONE token acknowledging the Attention signal.")`, so a
Stop during an `UPDATE` held by a trigger reported the attention as failed and
left the connection at an unknown point in the stream. Schemaic's grid commit
only trusts a rollback sent after an *acknowledged* attention, so every such
Stop was reported as a rollback that could not be confirmed.

**What.**

- `src/tds/stream/token.rs`, `flush_done_attention`: returns
  `Result<Option<TokenDone>>` — `None` when the message ended without the
  acknowledgement — instead of an error there.
- `src/client/connection.rs`, `cancel_request`: loops over messages, clearing
  `flushed` between them so the next one is read from the wire, until the
  acknowledgement arrives. A server that never sends one leaves it waiting;
  every caller in `schemaic_db::mssql` bounds it with `CANCEL_TIMEOUT`.
