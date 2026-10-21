# `db/cloud_iam.rs`

**`db/cloud_iam.rs` is the same process half for PostgreSQL and the MySQL family**, the pure half
being `core/cloud_iam.rs`. `token(cloud, host, port, user)` caches in memory for `db/entra.rs`'s
reason, in one `Mutex<Vec<Cached>>`, with one difference of shape: **an AWS token is its
target's**, signed for one host, port and user, so its entries are keyed on the host lowercased,
the port and the user and one is never handed to another target, where gcloud's — the account's —
is a single entry serving every connection
(`an_aws_token_is_handed_out_only_to_the_target_it_was_minted_for`). An entry is reused within
`Cloud::reuse_secs` of its fetch, and only while every identity file's modification time and
length — `None` for one that is absent — are what they were when it was cached, read after the
fetch as the Azure CLI's profile is, since asking can rewrite them (an SSO refresh, gcloud's token
store); so `aws configure` or removed credentials end its reuse
(`a_token_expires_from_the_cache_and_does_not_outlive_its_sign_in`). **There is no `forget` on a
refused login, unlike the Entra cache**: a changed CLI sign-in is caught by the identity key, not
by the server turning the token away. `fetch` is the Entra fetch again — `cli_candidates`,
`token_argv`, `tokio::process` with stdin null and `kill_on_drop`, `CREATE_NO_WINDOW` on Windows,
a 60 s `CLI_TIMEOUT` because gcloud's first run after an update can take tens of seconds — and
its failures are a `DbError::ConnectFailed(ConnectFailure::SignIn(..))` apiece, built by
`sign_in_failed` as `db/entra.rs`'s are: a CLI not found, `token_args`' or `token_argv`'s
refusal, a timeout, a CLI that could not be started, `failure` or `parse_token`'s refusal —
values the app words (under `core::model`), where they were English sentences inside a
`Connect`. **`Db::sign_in_secret` is its one caller**, and what MySQL's `dial` and
PostgreSQL's `connect_probe` hand their driver where the password goes: the password, or under an
IAM mode `AuthMode::transport_refusal` first — a `DbError::Refused`, with no CLI run, for a plan
short of the floor, the case being a handle assembled from parts — and then the token
(`an_iam_sign_in_without_tls_is_refused_before_any_token_is_asked`, red with the transport check
removed: the CLI was looked for and a Connect error came back). **The token is signed for
`Db::signing_target`, not the dial address**: an RDS token is a presign over `host:port` that
the instance checks against its own endpoint, and through an SSH tunnel the handle dials
`127.0.0.1:<local port>` — a token for that is refused, and `rds_region` reads no region from
it. So `Db::connect`'s tunnel arm carries the server's own `Connection::tcp_target` in the
`signing` field in the same step that moves the address, as it carries the certificate's name in
`hostname_override`, and the cache key is built from it too
(`a_tunnelled_iam_sign_in_signs_for_the_instance`); the MCP endpoint blob hands it on as
`signing_target`, rebuilt by `with_signing_target` (`the_endpoint_handoff_carries_the_signing_target`).
Not checked against a real RDS instance behind a bastion. On MySQL the token travels through
`mysql_clear_password`, which RDS and Cloud SQL ask for and `mysql_async` leaves off by default, so
`opts_with_tls` takes the secret and enables the plugin for the two IAM modes alone, their TLS
floor keeping the clear text inside an encrypted session; `opts`, which handed the stored
password itself, is `#[cfg(test)]` now. **What has been checked is narrow**: the live password
sign-in suites pass on MariaDB, MySQL 8, PostgreSQL 16 and SQL Server 2022 with the secret routed
through here, and with no AWS or Google Cloud account to sign in to, the IAM path is verified only
as far as invoking the CLI.
