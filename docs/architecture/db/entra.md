# `db/entra.rs`

**`db/entra.rs` is the process half of the Entra sign-in**, the pure half and why it is shaped as
it is being `core/entra.rs`. **It caches the token in memory**, because every `Db` operation
opens its own connection (*One connection per operation*) and the CLI takes a second or two to
answer, so a token per connection would put that in front of every query. One cache for the
process — the token is the CLI's signed-in user's, for one resource, whichever connection asks —
handed out until `MARGIN_SECS` (five minutes) before its expiry, so a connection is never given
one that dies on the way, or for `UNDATED_SECS` (ten) when the CLI printed none
(`a_token_is_replaced_before_it_expires`). It is never written anywhere; the CLI keeps the
refresh. **And only under the CLI sign-in it came from** (`reusable`): the token names whoever
was signed in when it was minted, and after `az login` as someone else, or `az logout`, the cache
went on handing out the previous identity's — which the server accepts, being valid, and
`forget` runs only on a refusal — for up to an hour, while the MCP subprocess, with its own cache,
could sign in as the other. So each entry carries a `CliIdentity`, the modification time and
length of the CLI's profile (`core::entra::azure_cli_profile`: `AZURE_CONFIG_DIR`, or `.azure`
under the home directory), which `az login`, `az logout` and `az account set` all rewrite; only
its metadata is read, never its contents, and it is taken after the fetch in case the CLI touched
its own profile. A profile that cannot be read keys nothing, and such a token is reused for
`UNKEYED_SECS` (a minute) rather than its hour
(`a_token_is_not_handed_out_under_another_cli_sign_in`). `fetch` finds `az` through `azure_cli_candidates`, spawns what `azure_cli_token_argv`
returns with stdin closed, bounded by `CLI_TIMEOUT` (60 s — a CLI's first run after an install
compiles its modules and can take tens of seconds), and on Windows with `CREATE_NO_WINDOW`, since
a GUI app's child otherwise gets a console window of its own, one flashing up per token. That
spawn is why this crate's `tokio` gained the `process` feature. The MCP subprocess is another
process and so has a cache of its own: it asks the CLI itself, and only the mode crosses in its
endpoint (under `ai.rs`).
