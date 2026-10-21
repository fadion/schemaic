# `app/copilot.rs`

`copilot.rs` — the `COPILOT_HOME` a GitHub Copilot session runs under, and the MCP config inside
it that gives the session our server. `CopilotHome::write(exe, Some(endpoint_file))` writes
`harness::copilot_mcp_config_json` into `instance_root("copilot")` as `schemaic-mcp.json` and
hands back the home (`write(exe, None)` hands back the same root with no config); `write_inline` makes `instance_root("copilot-inline")` and writes nothing
into it; `mcp_config()` is the path `turn_args` puts behind `--additional-mcp-config=@`; `env()`
is the one variable the child gets and `env_remove()` the three it must not inherit. `sweep`
collects the homes of instances that are gone, through `opencode::sweep_instances`, called from
`main.rs` beside `opencode::sweep` on the same background thread.
**The seal is a flag; the isolation is this directory, and that is the whole reason the module
exists.** Copilot's built-in tools are shut off on its own command line by `--available-tools`
(see `harness.rs`). What that flag does *not* do is stop the CLI loading the user's own
configuration — `~/.copilot/mcp-config.json`, whose servers it **starts** whether or not their
tools are then visible, their skills, their plugins and their hooks, and a hook is a command. The
flag that adds our server cannot displace any of that either: `--additional-mcp-config` says of
itself that it "augments config from ~/.copilot/mcp-config.json". `COPILOT_HOME` can — "override
the directory where configuration and state files are stored; defaults to `$HOME/.copilot`" —
and pointed at a directory Schemaic owns, none of the user's configuration exists for the
session. It is OpenCode's `XDG_CONFIG_HOME` again, and like that it edits nothing of the user's.
**Moving a CLI's home can log the user out, and off Windows this one would have.** Measured on
1.0.88 under Windows with an empty directory, a turn authenticated and answered — the token was
in the OS credential store. But `copilot login --help` says where it goes otherwise: *"If a
credential store is not found or there is an issue using it, the token will be stored in a plain
text config file under ~/.copilot/"*, which is every headless Linux box with no Secret Service.
So both `write` and `write_inline` call `carry_login`, which copies the user's `config.json` —
the file the CLI heads "This file is managed automatically", holding the login and nothing it
reads as configuration — from their own Copilot home (`user_copilot_home`: their `COPILOT_HOME`,
else the platform home's `.copilot`) into ours; settings, MCP servers, skills and hooks live in
other files and stay behind (`only_the_login_is_carried_into_the_home`,
`the_login_is_read_from_where_the_user_s_copilot_keeps_it`). It is best-effort: nothing to copy
leaves the CLI's own authentication error to surface on the first turn. A Windows home seeded this
way was measured to answer as the empty one did; the keychain-less path itself was not measured.
`env_remove()` is written to the variables:
`COPILOT_ALLOW_ALL` (auto-approves every tool, and its exact value `"true"` also trusts the
working directory, loading that directory's skills, plugins, MCP servers and hooks),
`COPILOT_ASSISTED_APPROVAL` (swaps the approval policy for a model's judgement) and
`COPILOT_CUSTOM_INSTRUCTIONS_DIRS` (adds the instruction directories `--no-custom-instructions`
is there to keep out) are cleared, each because it reopens something the argv closed; the
`COPILOT_PROVIDER_*` and token variables are deliberately **not**, because they are how a user
authenticates or brings their own model and clearing them would log the session out rather than
isolate it. `ai::inherited_env_to_clear` chains this list after OpenCode's and every harness's
child is cleared of both. `every_variable_that_widens_a_session_is_cleared` asserts the whole
slice, for the reason `OpenCodeConfig`'s own test gives — a sample lets one entry be dropped with
the suite green — `authentication_survives_the_isolation` holds `COPILOT_GITHUB_TOKEN`,
`GH_TOKEN`, `GITHUB_TOKEN`, the provider variables, `HOME` and `USERPROFILE` out of it, and
`the_home_is_the_lever_and_is_never_also_cleared` holds `COPILOT_HOME` itself out of it.
**Reused within an instance, because the session lives in it.** Copilot is a process per turn and
each turn resumes the previous one by id, from state the CLI keeps under `COPILOT_HOME`
(`session-state/`, `session-store.db`). A home per *session* would work; a home per *turn* would
forget every conversation after one question. So the root is per instance, from
`opencode::instance_root`, for the reason that function gives: the config carries the endpoint
file's path, which differs per connection, and a second window must not re-point the first one's
next turn. The consequence is OpenCode's too — the directory outlives the session, so the config
names the per-session endpoint file by *path* and the credential never enters it. The file is
written plainly, since it holds no secret and is rewritten per session. It is named
`schemaic-mcp.json` and not `mcp-config.json`, the name Copilot reads *by default* from its
home: naming ours by flag keeps "which servers does this session have" a question the argv
answers, rather than one that depends on which file a build happens to look for.
**A one-shot gets a home of its own, and it holds nothing.** `copilot-inline` is a separate root
for OpenCode's reason, a session's configuration and a one-shot's must not be one directory
whichever wrote last: there is no config file on the inline side at all, but a one-shot running
in the session's home would find that session's `schemaic-mcp.json` lying there, and a build
that read it by default would hand a server to the one path with nowhere to show a tool call.
`a_session_and_a_one_shot_never_share_a_home` asks it of `instance_path_in` over a fixture path,
never of `instance_root`, which creates directories under the real `config_dir()` — and holds
both apart from OpenCode's `opencode` root as well.
**`write` returning `None` refuses the session, as OpenCode's does, but for a reason of this
harness's own.** The allowlist is on the command line and would still hide every foreign tool
without the home; what does not survive is the isolation, and without it the CLI starts every
server in the user's `mcp-config.json` and loads their hooks, which is nothing a SQL assistant
should do on their behalf. So `ai::start_ai_session` refuses (through `refuse_every_turn`, like
the other non-spawning returns) and `ai::inline_env` refuses a one-shot whose `write_inline`
fails. Where the *endpoint file* is what could not be written, the session is not refused: it
runs with a server-less home and says it has no database tools — Codex's shape rather than
OpenCode's, since here the seal does not depend on the file that is missing. That home is the
*session's* root, `write(exe, None)`, not `write_inline`'s: a session with no tools is still a
conversation to resume next turn, and `copilot-inline` exists so the two never share a
directory. An earlier session's `schemaic-mcp.json` may still be lying in the root; nothing reads
it, because the file is named only by the flag and the flag is not passed without a config.
