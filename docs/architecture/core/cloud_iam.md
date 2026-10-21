# `core/cloud_iam.rs`

`core/cloud_iam.rs` — **signing in to PostgreSQL and the MySQL family with a cloud's IAM
token**: the pure half of `AuthMode::AwsIam` (an RDS token from the AWS CLI) and
`AuthMode::GcpIam` (a Cloud SQL one from the Google Cloud CLI), `db/cloud_iam.rs` being the half
that runs it. It is `core/entra.rs`'s shape, and for its reasons: the CLI holds the long-lived
sign-in and its refresh, a short-lived token is asked for when a connection needs one, and
nothing here stores one — the token goes where the password would. `Cloud` (`Aws`, `Gcloud`)
carries the CLI's name as a message says it (`cli()`) and how long a fetched token is reused
(`reuse_secs`: ten minutes of an AWS token's fifteen, fifty of gcloud's hour — a margin for the
clock and the connect). `cli_candidates` is `azure_cli_candidates` again: every absolute `PATH`
directory in order, then on Windows the installers' own folders, which find a CLI installed after
the app started — `Program Files\Amazon\AWSCLIV2\aws.exe`, and the Cloud SDK's
`Google\Cloud SDK\google-cloud-sdk\bin\gcloud.cmd` under `%LOCALAPPDATA%` or Program Files
(`the_candidates_cover_path_and_the_installers`). `rds_region` reads the region off an RDS
endpoint — `db.abc.eu-west-1.rds.amazonaws.com`, and the `.amazonaws.com.cn` spelling — and
answers `None` for any other host, a custom DNS name or an address, where the CLI's configured
default region applies (`an_rds_endpoint_names_its_region`,
`any_other_host_leaves_the_region_to_the_cli`). **There is no region field on the connection**,
so behind a custom name the CLI's default is the only way to name one.
**`token_args` puts connection-supplied values on a sign-in CLI's argv, which `entra`'s never
did.** AWS's is `rds generate-db-auth-token --hostname <host> --port <port> --username <user>`
plus `--region` where `rds_region` found one, the host and user each through
`launch::cli_flag_value` — a user named `--profile=prod` is an option to the CLI, not a user
(`the_aws_ask_names_the_host_port_user_and_region`, `a_value_shaped_like_an_option_is_refused`).
gcloud's is the constant `sql generate-login-token`: its token is the account's and names neither
the user nor the instance, the server reading the user off the connection
(`the_gcloud_ask_is_constant`). **`token_argv` keeps `launch`'s first rule as
`azure_cli_token_argv` does, and refuses where there is no way round the shell.** The Cloud SDK's
`gcloud.cmd` runs the Python it bundles over `lib\gcloud.py`, so that is what runs here —
`<sdk>\platform\bundledpython\python.exe <sdk>\lib\gcloud.py`, the SDK being the shim's `bin`'s
parent — and a shim without both is refused rather than handed to `cmd`
(`a_gcloud_shim_runs_the_sdks_own_python_and_nothing_else`). An `aws.cmd` is the old
pip-installed version 1 and is refused with that reason, version 2 being an `.exe`
(`an_aws_v1_shim_is_refused_and_an_exe_runs_as_found`). An `.exe`, or the CLI on any other OS,
runs as found — a script there, as `az` is. `parse_token` takes exactly one non-empty line with
no whitespace in it and refuses anything else: a warning or a prompt on stdout is not a token,
and accepted it would go to the server as the password (`a_token_is_one_line_and_nothing_else`).
`failure` is `entra`'s: a signed-out CLI says to run `aws configure` or `aws sso login`, or
`gcloud auth login`, and anything else keeps the CLI's own first stderr line
(`a_signed_out_cli_says_how_to_sign_in`). Every refusal here — `token_args`', `token_argv`'s,
`parse_token`'s and `failure`'s — is a `model::SignInFailure`, as `entra`'s are, but not every
one names a CLI: `failure`'s `SignedOut`/`NoTokenFrom` (and `db::cloud_iam`'s `NotFound`) take
it through `From<Cloud> for TokenCli`; `token_argv`'s `Shim` spells `TokenCli::Aws`/`Gcloud`
directly; `parse_token`'s `NoToken`/`MoreThanToken` and `token_args`' `BadValue` (from
`launch::cli_flag_value`, naming the field) carry no CLI. `identity_files` names what the token
cache keys
on, as `azure_cli_profile` does for Entra: AWS's `~/.aws/credentials` and `~/.aws/config`, or the
files `AWS_SHARED_CREDENTIALS_FILE` and `AWS_CONFIG_FILE` name; gcloud's `active_config`,
the **active** configuration's own `configurations/config_<name>` and `credentials.db` under
`CLOUDSDK_CONFIG`, else `%APPDATA%\gcloud` on Windows and `~/.config/gcloud` elsewhere
(`the_identity_is_read_off_each_clis_own_files`). The active configuration's and not the
default's, because `gcloud config set account` on a named configuration rewrites that file
alone when the account already has credentials: its name is `CLOUDSDK_ACTIVE_CONFIG_NAME`, else
`active_config`'s content (read through the caller's `read`), else `default`, and a name gcloud
would not make is not joined onto a path (`gcloud_is_keyed_on_the_active_configuration`).
