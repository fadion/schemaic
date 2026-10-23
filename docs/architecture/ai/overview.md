# `schemaic-ai`

`schemaic-ai` — agent-CLI sessions (stream-json) and turn parsing, for whichever CLI the user has
installed. `harness.rs` and `stream.rs` model six of them (`claude`, `codex`, `antigravity`,
`opencode`, `copilot`, `cursor`), and **all six are driven, with database tools**. Each is
selectable from the settings modal's *Agent CLI* dropdown, persisted as `UiState::ai_harness`,
carried on `AiSettings::harness`, resolved and interrogated by `app/agent_cli.rs`, and spawned by
`app/ai.rs` — Claude and Antigravity as one persistent child per conversation, Codex, OpenCode,
GitHub Copilot and Cursor as one process per turn.
**The dropdown's first entry is none of them**: *Disabled*, `harness::Agent::Disabled`, persisted
as `persist::AI_DISABLED_KEY` in the same field. A new install starts there and an upgrade does
not (`persist.rs`), and what off withholds is an invariant — *An assistant that is off sends
nothing and starts no agent process*.
**A fifth, `gemini`, was modelled here and refused, and it has now been deleted rather than left
as a menu entry that says no.** Google withdrew OAuth for personal accounts, so `gemini` needs an
API key to authenticate at all, and the CLI's own sign-in points at Antigravity as its successor —
which is a harness this crate does drive. Because the adapter was already refused rather than
driven, removing it costs no working session: the variant and its arms, `gemini_settings_json`,
the `stream.rs` dialect and every test of them are gone. It is written down because this is the
kind of decision that gets re-litigated by whoever next wonders why Google's CLI is not in the
picker. **The migration needed no code**, which is the whole point of the shape `from_key` already
had: it returns `None` for a key this build does not know, and `main.rs` warns and starts the
assistant off rather than coercing, so a `ui_state.json` still saying `"gemini"` takes the path
that was built for exactly this (`gemini_is_gone_and_its_persisted_key_resolves_to_nothing`).
`Constraint::notice` reaches the user in three places and no fourth: under the settings modal's
*Agent CLI* dropdown, where it describes the grade the selected binary would run at before a
session is started, in what `spawn_refusal` prints when the session is refused outright, and — since
the one-shot paths gained that same gate — in what `ai::inline_plan` hands back when it refuses.
In all three it is drawn as `schemaic_ui::i18n::constraint_notice_text`, the same sentence in the
interface's language (*The string catalogue*); `notice` itself stays the English. It
does not reach the AI panel itself, so a *runnable but weaker* session that was configured in an
earlier sitting carries no reminder. **Inline generation (Ctrl+K, AI Fill, AI Seed) runs on the
harness the picker names**, on that harness's own flags: `harness::inline_argv` is the closed
counterpart of `turn_args`, and Claude's arm of it is `inline_args`, which was the whole of this
path for as long as all three features spawned `claude` regardless of the selection. Why they did,
and the bug that made that the safe answer at the time, is under `app/agent_cli.rs`. A
harness key `main.rs` does not recognise is `tracing::warn!`ed and starts the assistant off.
**Getting the tools to each harness is six different mechanisms, and only Claude's is per
invocation.** Claude gets a `--mcp-config` file in a directory of Schemaic's own (`ai::mcp_dir()`,
and it was the temp dir until the credentials in it were moved somewhere only this user can
list). **That the flag is handed a *path* is an invariant, not a call-site habit**, and this crate
used to specify the other thing: `build_session_args`' parameter was `mcp_config_json`, its doc
said "passed to `--mcp-config`", and the test pinning the flag asserted inline JSON as the
expected argv value. The CLI accepts either form, and the config Schemaic hands it carried the
already-tunnelled database endpoint with its password (it names the saved connection now, and the
subprocess reads the password from the keyring — see `app/ai.rs`) — so the shape this crate
documented was the credential-carrying one, on argv, readable by every process listing on the machine. `app/ai.rs`
writes an owner-only private file and passes the file name for exactly that reason, and said so
only at its own call site; the parameter is `mcp_config_path`, and the doc and the fixtures say
path too. No behaviour changed — the one caller always passed a `PathBuf`.
Codex gets `-c` overrides on its own command
line; Antigravity has no per-invocation configuration at all and needs **two pieces of its own
global state** written instead — an `agy mcp add` registration *and* a per-tool
`permissions.allow` rule in its `settings.json` — which is what `app/antigravity.rs` owns and
removes. Headless mode cannot prompt, so a tool with no standing rule is auto-denied and the turn
still reports `"status":"SUCCESS"` with an empty response: a failure with no symptom. Measured end
to end against the real binary, with the four rules in place the same prompt returned real rows,
and `run_command` was never granted — which is what makes the CLI's own suggestion of
`--dangerously-skip-permissions` unnecessary rather than merely unattractive.
OpenCode has no configuration flag either (`--config` does not exist), but the state it needs is
**ours rather than the user's**: `app/opencode.rs` writes an `opencode.json` into a directory
Schemaic owns and points `XDG_CONFIG_HOME` at it, so one file carries both the MCP server and the
seal and nothing of the user's is edited.
GitHub Copilot splits the two that OpenCode folds together: its server arrives by a flag on every
turn, `--additional-mcp-config=@<file>`, naming a `schemaic-mcp.json` that `app/copilot.rs` writes
into a `COPILOT_HOME` of Schemaic's own, and its seal is a second flag, `--available-tools`, on
the same command line. The home directory is there for isolation rather than for the server — see
*Copilot displaces* below, and `app/copilot.rs`'s entry.
Cursor is configured by **the directory it is started in**: `app/cursor.rs` writes a project
`.cursor/cli.json` (the permissions file that is its whole restriction) and a `.cursor/mcp.json`
(our server) into a working directory of Schemaic's own, and runs `cursor-agent mcp enable
schemaic` there once per session, because a project server loads only once approved. It has no
flag that empties its tools and no sandbox that holds headless, so the file is not one lever among
several — see `harness.rs` for what it holds and what it does not.
**The isolation is not symmetric either, and Antigravity has none at all** — the one asymmetry the
list above would otherwise pass over while naming every other. Claude gets `--strict-mcp-config`
and Codex has its whole `mcp_servers` table assigned out from under it, and both of those
*displace* whatever servers the user has registered globally. `agy mcp add` appends to the user's
own MCP config, and no flag was found that scopes a registration to one invocation, so an
Antigravity session sees every server they have registered alongside ours. The per-tool
`permissions.allow` rules cover only the four `mcp(schemaic/<tool>)` names, so a user whose own
settings already allow their own servers' tools has those live inside Schemaic's SQL assistant.
Written down rather than fixed, because an undocumented gap is the one that gets assumed shut —
and it is asked as `harness::Harness::isolates_mcp_servers` so the surface that matters to the
*user* can read it too: `Constraint::notice` says in the panel that this harness's MCP surface is
not Schemaic's to restrict.
**OpenCode displaces them as well, and is the first that did it without touching anything of the
user's** (Copilot, below, is the second) — `XDG_CONFIG_HOME` pointed at Schemaic's own directory takes their registered servers
out of the resolved config entirely (measured: they vanish from `opencode debug config`), and
because nothing of theirs was edited there is nothing to put back. It does have a startup sweep,
but for a different currency: the roots are Schemaic's own, so a leftover costs disk rather than a
standing grant in somebody else's config. Note what
does **not** do this: `OPENCODE_CONFIG` and `OPENCODE_CONFIG_CONTENT` both **merge** with the
user's own file rather than replacing it, so a config naming only our server resolved to ours
*alongside* theirs. They are the obvious lever and the wrong one, which is why that is written here
rather than left for the next person to "simplify" towards.
**And rejecting them as a lever is not the same as being safe from them.** A child inherits the
parent's environment, so a user — or a shell profile, or whatever launched Schemaic — with
`OPENCODE_CONFIG` already exported gets their file merged straight back into a session
`XDG_CONFIG_HOME` was supposed to have isolated. Measured with both set at once: a foreign MCP
server appears alongside ours, inside a session the panel reports as `Sealed`. Setting our own
variables is half the seal and removing theirs is the other half, which is
`OpenCodeConfig::env_remove` and the `cmd.env_remove(..)` loop in `ai.rs` — see both entries below.
**Copilot displaces the user's servers by the same move, a home directory of ours.**
`COPILOT_HOME` pointed at a per-instance directory under Schemaic's config dir means the user's
`~/.copilot/mcp-config.json` — whose servers the CLI would otherwise *start*, whether or not
their tools were then visible — and their skills, plugins and hooks do not exist for the session,
and `--disable-builtin-mcps` keeps the bundled GitHub server from starting (measured on 1.0.88:
its status reads `disabled`). It keeps the login because the login is carried in: with an OS
credential store (measured on Windows with an empty directory) the token is not under the home at
all, and without one — headless Linux — the CLI's own help says it goes into a plain-text config
file under `~/.copilot/`, so every home is seeded with the user's `config.json` and nothing else
(`copilot.rs`). The lever that looks like the answer is the wrong one again — `--additional-mcp-config`
"augments config from ~/.copilot/mcp-config.json", so on its own it puts our server in beside
theirs. And the inherited environment carries levers of its own here too — an exported
`COPILOT_ALLOW_ALL` approves every tool — so `CopilotHome::env_remove` is the second list the child
is cleared of; `ai::inherited_env_to_clear` chains the two, and every
harness's child is cleared of both.
**Cursor cannot displace them, which makes it the second harness with no isolation.** Its
user-level `~/.cursor/mcp.json` is read from a path the CLI hard-codes to the home directory —
not `CURSOR_CONFIG_DIR` (read out of its bundle) — so the config-directory move that works for
OpenCode and Copilot is not available short of moving the home itself, which on macOS and Linux
would move the login with it. Measured (2026.09.23
build): a user-level server needs no approval and is **started on every run**; its tools are
listed to the model and refused when called, because nothing in Schemaic's permissions file
allows them — unless the user's own settings do. So `isolates_mcp_servers` is false for it as for
Antigravity, and its notice says so in its own words. `--approve-mcps`, which would load the
project server without the per-session `mcp enable`, is never passed: it approves every server in
reach, a user's plugins' among them.
**The spawned session is sealed to what Schemaic hands it, and that is `build_session_args`'s
three flags rather than `DISALLOWED_TOOLS`.** That statement is now *graded* rather than boolean,
because not every CLI can honour it in full — `claude`, `opencode` and `copilot` can, Claude and
Copilot by a flag each and OpenCode by configuration, and Codex, Antigravity and Cursor cannot.
`harness::Constraint` is the grade, and reporting
"sealed" for every harness would be the denylist-era bug a second time: a guard that reads
as total over a set nobody measured. The denylist named ten built-in tools and read like
the guard; measured against the shipped CLI by reading the `system`/`init` event's own `tools`
array, the old flag set left **nineteen** more live in the AI panel that it had never heard of —
`Artifact`, `CronCreate`, `CronDelete`, `CronList`, `DesignSync`, `EnterWorktree`, `ExitWorktree`,
`ListAgents`, `Monitor`, `PushNotification`, `RemoteTrigger`, `ReportFindings`, `ScheduleWakeup`,
`SendMessage`, `Skill`, `TaskOutput`, `TaskStop`, `ToolSearch` and `Workflow`. `Artifact`
publishes a page to the web, `SendMessage`/`PushNotification`/`RemoteTrigger` send outbound and
`CronCreate` schedules recurring agent runs: an assistant that had just read a client's rows was
one tool call away from publishing them. Enumerating what to *refuse* is the wrong shape — naming
what to *allow* is the right one, and after the change the same `init` probe reports
`tools: ["mcp__schemaic__run_query"]` with the MCP server still `connected` and a real turn
completing. `--tools ""` empties the **built-in** set and leaves the MCP allow-list untouched
(`the_session_is_given_no_built_in_tools_at_all` pins the empty string as the flag's whole value:
drop it and the next flag's name is read as a tool). `--strict-mcp-config` loads only the server
passed via `--mcp-config`, so the user's own global servers do not join Schemaic's SQL assistant.
`--setting-sources user` keeps the user's own `~/.claude/settings.json` — their own choice — and
drops the two directory-relative sources, because the CLI resolves them against the child's
working directory; the one-shot paths below inherit the app's own working directory instead, and
the flag covers them for the same reason. What it keeps is not inert: that file's `hooks` and
`apiKeyHelper` are commands, run with the child's environment, beside its `env` and its plugins —
whose skills may bring back what `--tools ""` removed, a plugin's MCP servers being
`--strict-mcp-config`'s to keep out — so the residual is a tampered *user* config rather than a
planted one, and it includes running commands.
**And the session child runs in a directory made for it, never in the temp dir itself.**
`ai::session_cwd` creates `<temp>/schemaic-run-<pid>-<started>-<tag>`, fresh per session, through
`create_exclusive_dir` — `DirBuilder::mode(0o700).create` on Unix, `fs::create_dir` elsewhere, and
deliberately **not** `create_dir_all`, because refusing an existing path *is* the security
property: `create_dir_all` succeeds on a directory somebody else made, and on a symlink into one.
`std::env::temp_dir` on Unix is world-writable, and `/tmp`'s sticky bit
stops another local account deleting your files, not taking a path nobody has: a
`.claude/settings.json` pre-created there has its `hooks` run as this user the next time the AI
panel opens. `--setting-sources user` closes that on a current CLI, but it is exactly the flag
`CliSeal` may have to drop, and unlike `--tools` — backstopped by `DISALLOWED_TOOLS` — nothing
stood in for it. Nobody can plant a file in a directory that did not exist a moment ago and that
only this user may write, so owning *this* directory makes the flag defence in depth exactly as
owning a tree of ours did.
**It was `persist::private_dir("ai-session")`, under the app's own config dir, and that was the
release review's one Critical (R3-L5-01).** That directory's *parent* holds `connections.json` —
every saved connection's host, user, database and SSH account, plus the plaintext DB password, SSH
password and key passphrase in the keyring-unavailable fallback — and `schemaic.log`. It was
chosen when Claude, which is `Sealed` and has no built-in tools, was the only harness; Antigravity
is graded `Restricted`, its filesystem readers are auto-approved headless, and this repository's
own measurements record a turn running `list_dir` and `view_file` unprompted. A table comment, a
column comment, an imported dump or a row value is text the user did not write, and
`../connections.json` was one relative path away. The credentials the AI paths *write* moved the
other way in the same change, out of the temp dir and into `ai::mcp_dir()` — see the `ai.rs` entry.
`session_cwd` answers `Option<PathBuf>`, trying the temp dir and then
`persist::private_dir("ai-run")` — owner-only by construction, and a private subdirectory rather
than the config dir beside `connections.json` that the paragraph above forbids — and `None`, when
neither yields a fresh exclusive directory, is a **refusal of the turn** (`no_cwd`, one wording for
every spawn path), not a spawn with no `current_dir`. That is what it used to be, and it is not
neutral: a child given no `current_dir` inherits Schemaic's own, which is where a CLI resolves
`.claude/settings.json` and `AGENTS.md` — the very thing the directory exists to keep away from it.
`no_agent_spawn_sets_its_working_directory_conditionally` scans `ai.rs` for the `if let Some`
spelling the three spawn sites used to have. All three sit **ahead of the variadic `--allowedTools`/`--disallowedTools`**, which would
otherwise swallow a later flag's name as a tool name, and both
`the_session_loads_no_mcp_server_and_no_config_dir_of_its_own_finding` and the test above pin that
order. **They did not prompt — they ran.** Driving the old flag set with
`--permission-mode default` and a `--permission-prompt-tool stdio` route deliberately left
unanswered, `Skill`, `ToolSearch` and `Monitor` all executed inside the turn and **no
`can_use_tool` control request was emitted at all**, in two separate trials. So the exposure is not
that these tools ask and go unheard; it is that they do not ask. Whether an un-allow-listed tool
can *also* stall a turn is a separate question this measurement does not answer. The only evidence
on it is the older `propose_table_change` observation recorded under `mcp.rs`, and that is about a
different class of tool: an **MCP** tool absent from `--allowedTools`, denied invisibly. These
nineteen are **built-ins**, which that allow-list never named and `DISALLOWED_TOOLS` did not
cover — which is precisely why they were reachable, and why nothing governed them until
`--tools ""` emptied the set. So the denial rule does not carry across to them, and neither does
its opposite: the measurement shows these three ran, not what the CLI would do with a built-in it
had decided to gate. The TODO item that reported a turn stalling with no message is
therefore **not** explained by these nineteen, and its cause is still unidentified; emptying the
built-in set was justified by what the tools can do, not by that symptom.
`DISALLOWED_TOOLS` is kept as a **backstop** in case a future CLI stops
honouring `--tools`, and it now names **all twenty-nine** — the original ten plus that measured
nineteen, in the order the `init` event reported them. It named only the ten for a while, under a
description claiming it covered the worst misuse, and a review found it omitted every tool the
same measurement had called worst: `Artifact`, `SendMessage`, `RemoteTrigger` and `CronCreate`
all walked past it, so the fallback fell back on the shell and the filesystem and on nothing that
mattered. It is still a **snapshot rather than a complete denylist** — a complete denylist is
precisely what failed, and this one can only ever name what somebody has looked at — but a
snapshot has to name the worst of what it can see: a name the CLI does not know is harmless in a
denylist, a name missing from it is not. Re-measured against the live CLI, all twenty-nine spawn
cleanly and the `init` event still reports `tools: []`.
**The three flags are passed only if the CLI accepts them, and that is asked of the binary rather
than of a version number.** Schemaic spawns whatever `claude` the user has and pins no version,
and a flag an older CLI does not know does not degrade the session, it kills it — the child exits
with `error: unknown option '--tools'` before the first turn and the AI panel is gone until the
user upgrades. So `agent_cli::probe` runs `<bin>` with that harness's own `Harness::help_args`
(`--help`, or `exec --help` on Codex — see the entry below for the isolation that hid on the
top-level page) and hands the **whole probe** —
the exit status and both streams — to `schemaic_ai::seal_from_probe`, which reads a
`CliSeal` out of it, one `bool` per flag. The status is half the answer and the caller must not
decide it alone: a `--help` that fails loudly still prints something, and that something is a
diagnostic rather than a help text, so reading the output on its own treated a failed probe as a
CLI lacking every sealing flag. A non-zero exit yields `CliSeal::ALL`; stderr is folded in beside
stdout only as a fallback for a CLI that prints its usage there.
`build_session_args` and `inline_args` both take one and
turn it into argv through the single private `seal_args`, so Claude's session and one-shot paths
cannot come to seal themselves differently — the one-shot paths being the ones with no surface
that would show it if they did. `CliSeal` is Claude's alone: the other five are sealed by their
own arm of `harness::turn_args` or `harness::inline_argv`, and `InlineSpec::seal` is carried past
them unread. A version number would only stand in for the answer `--help`
gives directly, and would need a table mapping releases to flags that nothing in this repository
can keep true.
**The failure direction is deliberate and is the security-relevant half.** An empty or unreadable
help text yields `CliSeal::ALL`, not `NONE`: a probe that failed for a transient reason on a
*working* CLI degrades to a spawn that may error loudly, never to a session quietly running
unsealed. Guessing "absent" on no evidence would silently reopen the hole this whole change
exists to close, with nothing on screen to say so
(`an_unreadable_probe_seals_rather_than_opens`). Only a help text that was actually read, and
that actually lacks the flag, drops it — and **"read" is now a positive test rather than
`!is_empty()`**. Emptiness was the only unreadable case the check knew, so a probe that caught a
diagnostic instead of help — an npm shim whose `node` is missing, a one-line usage from a wrapper,
an auth wall — was non-empty, mentioned none of the three flags, and was read as their absence.
`looks_like_help` requires the text to document at least one flag every build of the CLI has
always had (`--help`, `--print`, `--model`, `--version`), and **deliberately not one of the three
sealing flags**: their absence is the answer the gate is qualifying, so testing for them would
make it circular and it could never seal. On a CLI too old for `--tools`, `DISALLOWED_TOOLS` is what
still stands between the assistant and a shell — that is the case the backstop exists for, and
`an_old_cli_gets_no_sealing_flags_but_keeps_the_backstop` pins it.
**The `HELP` fixture is captured from the real `claude --help`, not written from memory** — the
same rule the engine-error fixtures follow, and for the same reason: this parser decides whether
the sealing flags are passed at all, so a fixture shaped the way help *ought* to look would prove
nothing about the binary Schemaic spawns. The capture is what supplied the real near-miss names:
the CLI lists two spellings of each tool-list flag on one line (`--allowedTools, --allowed-tools
<tools...>`) and mentions `--tools` inside another flag's prose, and
`a_cli_listing_none_of_the_flags_is_read_as_having_none` holds a help text of nothing but those
names to `CliSeal::NONE`. `mentions_flag` requires the character after the flag to end it rather
than using a bare `contains` — **not** because any name in today's help contains `--tools` (none
does; the near misses are all a dash short), but because these names are one rename away from
doing so, and the direction it would fail in is passing flags that kill the spawn. **That is why
the real capture cannot be the test of the check, and a second fixture is.** Because today's near
misses are a dash short, a `contains` answers the captured help identically — the test that held
only it asserted the check in its doc comment and exercised none of it, and was split once that
was noticed. `a_flag_is_not_found_inside_a_longer_flags_name` supplies the collision the capture
cannot (`--toolsmith`, `--setting-sources-file`, and neither of ours) and fails against a
`contains` variant with `tools: true, setting_sources: true`; the captured fixture keeps its own
narrower claim and says in its doc that it is not the check's test. Its descriptions deliberately
spell no bare flag, since `mentions_flag` reads prose too and a first draft passed on that
instead. It matches
anywhere in the text, prose included, so a help that merely *discusses* `--tools` in another
flag's description reads as advertising it — an imprecision kept because it errs towards sealing,
which is the direction every other choice here errs in too. The
probe costs **~140 ms measured**, so `probe` caches the answer per `(harness, resolved path)` —
every outcome but a spawn that never ran, which is a property of the moment rather than of the
file (see `agent_cli.rs`) — and
`main.rs` calls `warm_probe_cache` on a background thread; without it that wait
sits on the UI thread ahead of every AI action. **What it warms must be what the spawn will
resolve** — `harness_bin(h, &ai_cli_path)`, not `detect_bin(h)`: the cache is keyed by the
resolved path, and an AI CLI override makes those two different keys, so warming the auto-detected
one warmed an entry nothing ever read and left all four AI entry points paying a blocking `--help`
on the UI thread. It is therefore called from an **effect on `harness_bin(h, &ai_cli_path)`**
rather than once at startup, so changing the override re-warms the key that will actually be
used — and the effect reads the **harness** signal as well, since that is the other half of the
key and a second way to warm an entry nobody reads. **It returns early while the assistant is
off**, because the probe *is* a spawn of the CLI and runs at every launch: off has to mean no agent
process at all, not only no question sent. It reads `ai_enabled` tracked, so turning the assistant
on is what warms the cache. **It warms once per key, not once per run**: it returns the
`(Harness, resolved bin)` it warmed and skips a run that would warm the same one. A Floem `set`
notifies even for an unchanged value, and the harness-switch effect writes `ai_cli_path` back on
its first run, so this effect ran twice at every launch, both times before the first probe had
filled the cache, and started two `claude --help`s (*Floem 0.2 gotchas*).
**One key now covers every AI entry point, and it took two goes to get there.** Ctrl+K, AI Fill and
AI Seed used to probe `(Claude, <claude bin>)` whatever was selected, which is a different key from
`(selected harness, its bin)` whenever the selection is not Claude — so with Codex, Antigravity or
OpenCode chosen, every Ctrl+K paid a blocking, timeout-free `claude --help` on the Floem UI thread.
That was verbatim the regression this function's own doc says it prevents, reintroduced through a
second key rather than through the path, and the effect was made to warm the Claude entry alongside.
The one-shot paths build their own harness's argv now (`ai::inline_plan`), so they read the key the
effect already warms and the second warm is **deleted rather than kept as insurance** — an entry
nobody reads is the thing this whole passage is about.
**The one-shot argv is sealed the same way a session's is, and its case is sharper.** Ctrl+K, AI
Fill and AI Seed each want one string back that a parser then reads, and none of them has a surface
that could render a tool call — so where the chat panel merely stalls on a tool it cannot prompt
for, those three would have run one *invisibly*. So `harness::inline_argv` hands no harness a
server: no `--mcp-config` (Claude's `--strict-mcp-config` then leaves it with none), no `-c`
carrying `mcp_servers` on Codex, an OpenCode config with the `mcp` block left out, on Copilot
no `--additional-mcp-config` and an allowlist naming a tool nothing provides, and on Cursor a
working directory of its own whose permissions file allows nothing and which has no `mcp.json`
at all. The whole
request is in the prompt (`no_inline_generation_is_given_a_server_or_a_session` over
`Harness::ALL`, `a_one_shot_generation_is_given_no_tools_and_no_servers` for Claude's arm, and
`inline_args_flags_in_order` holds its exact argv). **On Antigravity that is a statement about the
argv and not about the run**: its server is registered globally, and a one-shot fired while a chat
session holds that registration sees it — measured, and written up under `harness.rs`'s
`inline_argv` entry with the reason no flag there can take it away.
**A command line too long to spawn is refused before the spawn, and the refusal counts in the
platform's own units.** `arg_limit` is 30,000 on Windows (headroom under `CreateProcess`'s 32,767
for the executable path and the runtime's quoting) and 128 KiB elsewhere, where the binding
constraint is `MAX_ARG_STRLEN` on a *single* argument — and the system prompt, carrying the schema
outline, is a single argument. `oversize_reason` is checked ahead of every spawn because the
failure it prevents is unrecognisable afterwards: the OS returns a generic error and the app's
handler sends the user off to check an installation that is fine. It measures with the private
`arg_units`, which is **UTF-16 code units on Windows and bytes elsewhere**. It was `str::len()` —
UTF-8 bytes — on both, while `arg_limit`'s own doc correctly calls the Windows cap *characters*,
which for `CreateProcessW` means UTF-16 units: a CJK schema outline costs three bytes and one unit
per character, so a ~12,000-character outline was refused at "36,0xx characters" when the real
command line was ~12,000 units. Conservative in direction, but the number in the refusal was wrong
by 3× and the one lever it suggests — narrowing *Settings → AI → Schema context* — could not
close a gap that was not there. Elsewhere `MAX_ARG_STRLEN` really is bytes and `str::len()` is
exact, which is why this is a per-platform answer rather than one unit for both
(`an_argument_is_measured_in_the_units_the_platform_caps`).
**The refusal names that unit too, and fixing only the measurement left the wording behind.** The
message said *"({total} characters; this platform allows about {limit})"* on every platform, so
once the count was right it was a byte count labelled "characters" everywhere but Windows — the
same error one step further on, and the step that reaches the user. A number in a refusal is only
useful if the reader can compare it to the limit printed beside it, and they cannot compare two
figures whose unit they have been told wrongly. `ArgUnit::PLATFORM` answers `Characters` on
Windows and `Bytes` elsewhere (`name()` the English word) — the count and the word that labels it
reading the same `cfg!(windows)`, `arg_units` below and the constant above, so a platform added
to one has to be added to the other in the same edit. It was `arg_unit_name()`, a `&str`, until
the refusal became a value the window words (below).
**And the lever the refusal names is the caller's, because it is not the same everywhere.**
`oversize_reason` and `spawn_refusal` take `levers: Levers`, of which there are two constants:
`SCHEMA_AND_QUERY_LEVERS` (narrow *Settings → AI → Schema context*, or shorten the query in the
editor) for a chat turn or a Ctrl+K generation, and `SAMPLE_LEVERS` (lower this connection's AI
data access below *"Let it read data"*, which stops the row sample being sent, or narrow the same
*Schema context*) for AI Fill and Seed — each naming a setting in its own on-screen words, so the
reader can find it. Those two are grid context
menus: there is no editor and no query, so the chat wording gave the user nothing to act on while
the one thing that would have worked went unnamed. `ai::inline_plan` threads the argument through
to the one-shot paths, and `an_oversize_refusal_names_the_levers_its_caller_has` asserts both
messages still carry the measurement and still blame no installation.
**The refusal is a value, not a sentence.** `oversize_reason`, `batch_shim_reason` and
`spawn_refusal` answer `Option<SpawnRefusal>` — `Oversize { harness, total, unit, limit, levers }`
or `BatchShim { harness, bin }` — and `cli_failure_message` answers a `CliFailure`: `Said`, the
CLI's own stderr or else stdout passing through, or the two fallbacks that are Schemaic's,
`Exited { harness, code }` and `Signalled { harness }`. Each `Display` is the English byte for
byte, `Levers::message()` the two lever sentences; the constants `SCHEMA_AND_QUERY_LEVERS` and
`SAMPLE_LEVERS` are `Levers` now, so no caller changed. They used to be finished English
`String`s, which reached the AI panel, Ctrl+K and the grid's Fill and Seed in English whatever
the interface's language; the window words them through `schemaic_ui::i18n` (*The string
catalogue*).
