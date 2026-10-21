# `core/entra.rs`

`core/entra.rs` — **signing in to SQL Server with a Microsoft Entra token from the Azure CLI**:
the pure half of `AuthMode::AzureCli`, `db/entra.rs` being the half that runs it. The token is
the CLI's to mint and refresh — `az account get-access-token --resource
https://database.windows.net/` (`AZURE_SQL_RESOURCE`, the token's audience) answers with one for
whoever is signed in to it — and nothing here stores one. What lives here is what can be tested
without a CLI. `azure_cli_candidates` is where to look: every absolute `PATH` directory in order
(a relative entry skipped, for `agent_cli::which_on_path`'s trailing-`;` reason), then on Windows
the installer's own `%ProgramFiles%\Microsoft SDKs\Azure\CLI2\wbin\az.cmd`, which is what finds a
CLI installed after the app started and so missing from the `PATH` it inherited — seen on the
machine this was written on (`az_is_looked_for_on_path_then_where_the_installer_puts_it`).
**`azure_cli_token_argv` keeps `launch`'s first rule, and on Windows that takes a detour.** `az`
there is `az.cmd`, a batch file, and `CreateProcess` runs one through `cmd.exe`; the installer's
shim is two lines running the Python it ships, `"%~dp0\..\python.exe" -IBm azure.cli %*`, so this
runs that Python directly — found beside the shim (a virtualenv's `Scripts`) or one folder up (the
installer's `wbin`) — and a shim with no Python where one should be is **refused** rather than
handed to `cmd`, as `launch::direct_spawn_verdict` refuses one. Elsewhere `az` runs as found; it
is a script there too, the same named exception `xdg-open` is in `launch`, and every argument is
a constant (`the_token_request_runs_with_no_shell_in_between`). `parse_token` reads the CLI's JSON
— `accessToken`, and `expires_on` as an epoch number or, on some versions, a string — and refuses
anything else rather than guessing (`a_token_is_read_from_the_clis_answer`). **`AzureToken`'s
`Debug` is hand-written and redacts the token**, a bearer credential, for `Db`'s reason (*Connection
identity*, under the invariants); the same test asserts it never prints. `failure` turns the
CLI's stderr into something a user can act on: a signed-out CLI is `SignedOut`, which says to
run `az login`, anything else `NoTokenFrom` with the CLI's own first line without its `ERROR:`
prefix, or with none where it printed none (`a_failure_says_what_to_do`). That, the shim
refusal and `parse_token`'s two are a `model::SignInFailure`, not a sentence, so the interface
words them (under `core::model`); `to_string()` is the English they were.
`azure_cli_profile` names the file whose change means the CLI's sign-in changed —
`AZURE_CONFIG_DIR`'s `azureProfile.json`, or `.azure`'s under the home directory — which
`db/entra.rs` keys its token cache on
(`the_cli_profile_is_under_its_config_dir_or_the_home_directory`).
