# `app/cursor.rs`

`cursor.rs` — the working directory a Cursor session runs in, the two files in it that restrict
and configure the session, the one approval the CLI needs before it will load our server, and
the sweep that takes back what the CLI keeps about that directory in the user's own Cursor
config. `CursorWorkspace::write(exe, Some(endpoint_file), allowed)` writes
`harness::cursor_permissions_json(allowed)` as `.cursor/cli.json` and
`harness::cursor_mcp_config_json` as `.cursor/mcp.json` under `instance_root("cursor")`;
`write(exe, None, allowed)` writes the permissions file alone and removes any `mcp.json` an
earlier session left, so the CLI is not handed a server pointing at a file that no longer
exists; `write_inline` writes the same denials with nothing allowed and no server under
`instance_root("cursor-inline")`. `root()` is where the child is started and `has_server()`
whether `approve` is owed. `sweep` runs from `main.rs` after `copilot::sweep`, on the same
background thread.
**The restriction is a file, and a missing file fails open.** Cursor has no flag that empties its
tools and no sandbox measured to hold headless; what holds is the project permissions file, and
what it holds and what it does not is under `harness.rs`. Measured on the 2026.09.23 build: with
no file at all a headless turn refused its shell and every MCP call, and **wrote a file
unprompted**. So `write` returning `None` refuses the session in `ai::start_ai_session` (through
`refuse_every_turn`, like the other non-spawning returns), and `ai::inline_cwd` refuses a
one-shot whose `write_inline` fails — OpenCode's position, for a more literal reason: there the
missing file leaves a shell, here it lets the writers through. Where only the *endpoint
file* is missing the session is not refused, because the permissions file is still written.
**The working directory is the configuration.** Both files are *project* files, found from the
directory the CLI is started in, so the child's `current_dir` is not only a guard against a
planted file here, as it is for the others, but the lever itself — which is why `ai.rs` starts
this harness somewhere other than `session_cwd`, and why `env_seal` is false for it. **Per
instance and reused, because the conversation lives in it**: the CLI keys a conversation's saved state by the directory it ran in
and each turn resumes the last, so a directory per *turn* would lose the conversation and one per
session would scatter entries across the user's Cursor config. The root comes
from `opencode::instance_root` for that function's reason — the files carry a per-connection
endpoint path and the access level's tools, and a second window must not re-point the first
one's next turn — and the consequence is OpenCode's too: the directory outlives the session, so
`mcp.json` names the endpoint file by path and the credential never enters it. The one-shot's
root is separate for Copilot's reason, so a live session's `mcp.json` is never lying in a
one-shot's directory (`a_session_and_a_one_shot_never_share_a_workspace`, over
`instance_path_in` and a fixture path).
**One approval, and only ours.** A project MCP server loads only once approved — measured, *"not
loaded (needs approval)"* and every call refused until then. `approve(launch, workspace)` runs
`cursor-agent mcp enable schemaic` in the workspace, through `agent_cli::Launch` like every other
spawn of this CLI, with stdin null and a 60-second ceiling (`APPROVE_TIMEOUT` — the measured runs
took a few seconds, and a hung one must not hold the session). **Both pipes are drained on
threads while it runs**: the first version read neither until the child exited, and a child
blocked on a full pipe never exits, so a chatty failure would have hung to the ceiling and lost
the message `cli_failure_message` is called to show. After the exit the output is waited for
two seconds at most, since the `.cmd` fallback's node can still hold a write end. What it
reports is `approve_verdict`, pure over how the run ended
(`a_failed_approval_carries_the_clis_own_words`). It answers an `ApproveFailure` (`Unfinished`,
`Cli(CliFailure)`, `Io`), not a string, because the one failure has two readers:
`schemaic.log` takes its `english()`, and the panel's note takes its `shown()`. One worded
string served both, and the log line a bug report attaches carried the interface's clause
(`english_says_every_approval_failure_as_the_log_does`). The record it writes is keyed by
a hash of the config's contents — changing the endpoint path read *"needs approval"* again — so it
is asked again every session. **`--approve-mcps` is the lever that looks like the answer and is
never passed**: it approves every server in reach, a user's plugins' among them, where this
approves ours by name. A failure is a session with no database tools that says so, Antigravity's
shape for a registration that failed.
**What the CLI keeps is Antigravity's position, and the sweep is `antigravity::sweep`'s analogue
with one difference that makes it safe.** The approval record and the conversations land in the
user's own Cursor config directory, resolved by the CLI's own rule read out of its bundle —
`CURSOR_CONFIG_DIR`, else `$XDG_CONFIG_HOME/cursor`, else `<home>/.cursor` (`cursor_config_dir`):
`projects/<slug>/`, whose `.workspace-trusted` records the `workspacePath` and which holds the
approvals and transcripts, and `chats/<hash>/<chat>/`, whose `meta.json` records the `cwd`. That
is global state outliving the process, entries for every instance that ever ran for as long as
nothing takes them back. **Every entry is found by its content naming one of Schemaic's own
directories, never by recomputing the CLI's slug or hash**: `workspace_pid` accepts a recorded
path only when it is exactly `<the cursor or cursor-inline base>/pid-<n>` — compared
case-insensitively on Windows, where the CLI records whatever its process reports — so nothing
of the user's own can match, not even a project of theirs named `pid-42` or one they put *under*
our directory (`only_our_own_workspaces_are_recognised`, which also holds OpenCode's roots out
of it). The entry then goes only when that pid has no live process — `opencode::sweep`'s rule,
any process on the pid, and not `antigravity.rs`'s start-time claim — so a recycled pid keeps an
entry until that process ends. The trade is OpenCode's, leftovers kept longer rather than a live
session broken, and what is kept is state recorded against a directory of Schemaic's rather than
a grant in the user's own settings. A `chats` group directory is removed only if empty and only
where this sweep emptied it; each removal is best-effort and never touches anything else.
Then `opencode::sweep_instances` collects the workspaces themselves. **The user's directory is
walked only when one of ours is dead**: `chats/` holds one entry per conversation the user has
ever had with the CLI, so the sweep first lists its own bases and returns before opening a single
`meta.json` unless some `pid-<n>` there has no live process. An entry of ours outlives its
workspace only if this sweep failed to remove it, the state being removed first and the
workspace second — and **such an entry is not retried at the next launch**: the workspace goes
regardless, so the walk that would find the entry again runs only when some later instance's
workspace is found dead at startup, which for a user who stops picking Cursor is never. The cost
is a conversation directory left in the user's Cursor config after a removal failed (a file held
open by a still-running `cursor-agent`, a scanner); keeping the unswept workspace to force the
retry would change what the sweep deletes, and is left open. `cursor_config_dir`'s precedence is
`cursor_config_dir_from`, pure over its three values, and the Windows case rule is
`workspace_pid_folding`'s argument, so both are exercised on every platform
(`the_cursor_directory_is_found_where_the_cli_looks_for_it`,
`a_workspace_recorded_in_another_case_matches_only_where_case_folds`). A recorded path spelled
with the other separator or a trailing one does not match — the fail-safe direction.
**And what cannot be closed.** The user's `~/.cursor/mcp.json` is read from a path the CLI
hard-codes to the home directory, and moving the home would move their login with it on macOS and
Linux; its servers need no approval and are started on every turn. Their tools are refused
unless the user's own settings allow them, because no allow rule here names them — which is
`isolates_mcp_servers` false, and the notice saying so.
