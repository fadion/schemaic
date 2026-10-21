# `app/opencode.rs`

`opencode.rs` — the config directory an OpenCode session is sealed by, and the environment that
points the CLI at it. `OpenCodeConfig::write` puts `harness::opencode_config_json` on disk and
hands back the root, `write_inline` does the same for a one-shot with
`harness::opencode_inline_config_json`; `env()` is what the child `Command` gets, `env_remove()`
is what it must *clear*; `root()` is for logging and tests.
**The two configs get separate roots, and that is not tidiness.** Both are the file
`<root>/opencode/opencode.json`, so one shared root would leave whichever ran last deciding
whether a server is registered — a Ctrl+K quietly stripping the server from a chat session running
beside it, or a chat session handing its server to the one path with nowhere to render a tool
call. `write` takes `instance_root("opencode")` and `write_inline` takes
`instance_root("opencode-inline")`; two roots cannot race. That was argued in a paragraph and
asserted nowhere, and the decision is two string literals inlined between a `create_dir_all` and
an `fs::write` — changing `"opencode-inline"` to `"opencode"` is a plausible tidy-up, since the
subdirectory under each is already `opencode/`, and it would leave a Ctrl+K during a live chat
overwriting that session's config with the server-less inline one, with the suite green.
`no_two_configs_that_must_differ_can_land_on_one_path` pins it, and pins the instance component
below with it. The inline root is reused on the same
terms as the session's, and `harness::inline_argv` passes `--pure` for the same reason `turn_args`
does. `write_inline` returning `None` must refuse just as hard, and for the same reason —
`--agent schemaic` naming an agent that does not exist runs on `build`, which has `bash` — which
is the refusal `ai::inline_plan` makes.
**A directory rather than a file, because the file levers are additive.** That CLI has no
`--config` flag, and the two environment variables that look like the answer are not one:
measured against the installed binary, `OPENCODE_CONFIG` and `OPENCODE_CONFIG_CONTENT` both
**merge** with the user's own `~/.config/opencode/opencode.json`, so a config naming only our
server resolved to ours *and* theirs. `XDG_CONFIG_HOME` is the one that replaces — pointed at a
directory of ours, the user's globally registered servers vanish from `opencode debug config`
entirely. That is this harness's `--strict-mcp-config`, and unlike `antigravity.rs` it edits
nothing of the user's, so there is no `Drop` and no teardown here — the directory is still ours
to collect, which is what `sweep` below does, and the difference is that leaving one behind costs
disk rather than a standing grant.
**Rejecting those two as the lever left them able to reopen the seal, which is what `env_remove`
is for.** Setting `XDG_CONFIG_HOME` says nothing about what the child *inherits*, and both merging
variables merge just as well when the parent's environment supplied them: measured with
`OPENCODE_CONFIG` and `OPENCODE_CONFIG_CONTENT` exported alongside our own, a foreign MCP server
resolves beside ours inside a session reported as `Sealed`. `env_remove()` names those two and
`OPENCODE_CONFIG_DIR`, and `ai.rs` calls `cmd.env_remove(..)` for each **before** `.envs(..)`;
after the fix only our server resolves. The third did not move the resolved config when measured
on its own — it is cleared because it is a config-redirection variable by name, and leaving it for
the next person to re-measure is exactly how the other two got missed. What is deliberately **not**
cleared is anything under the data directory or `HOME`, for the reason the next paragraph gives
about `XDG_DATA_HOME`: `auth.json` lives there, and taking it away logs the user out of their own
CLI rather than isolating anything
(`nothing_the_seal_depends_on_is_cleared_by_accident` holds both halves at once: everything `env()`
sets survives `env_remove()`, and `XDG_DATA_HOME`/`HOME`/`USERPROFILE`/`OPENCODE_API_KEY` are
untouched; `every_config_lever_the_module_rejected_is_cleared_from_the_child` is the other
direction, and it asserts the **whole slice** now rather than the two entries it used to check —
with `OPENCODE_CONFIG_DIR` unasserted it could have been dropped or misspelled with the suite
green, which is exactly what that entry's own doc warns is how the other two got missed).
**`env()` returns `OsString`s, and that is the third way the seal could silently open.** It
returned `String`s built with `to_string_lossy`, so a config root that is not valid UTF-8 — a
home directory in a legacy encoding, a Windows path with an unpaired surrogate — pointed
`XDG_CONFIG_HOME` at a directory with the invalid bytes replaced by U+FFFD: *not* where the
config was written. `--agent schemaic` then names an agent that does not exist, OpenCode falls
back to its own `build` agent with every built-in including `bash`, and the panel reports
`Sealed`. `write`'s contract is that a failure to seal must **refuse**, and it is honoured for a
failed write and for a missing `private_dir`; a root that could not survive a `String` round trip
was the one way past it. `Command::env` takes `AsRef<OsStr>`, so the round trip was imposed by
the signature alone. `ai.rs`'s `InlinePlan::env` and the per-turn `seal_env` carry the same type, and
`nothing_the_seal_depends_on_is_cleared_by_accident` now also asserts that the value reaching the
child is the root path itself rather than a rendering of it.
Three further details in it are load-bearing. **Only `XDG_CONFIG_HOME`, never `XDG_DATA_HOME`** — the two
are easy to set together and the second breaks the session, because `auth.json` lives under the
*data* directory and moving it logs the user out of the CLI they just signed into, failing on
credentials rather than on anything Schemaic visibly did. **`OPENCODE_DISABLE_PROJECT_CONFIG=1`**
closes the second axis, the `opencode.json`/`.opencode/` that CLI also reads from its working
directory; `ai::session_cwd` is a directory created for that one session and empty when the CLI
starts in it, so this is defence in depth behind owning the cwd, exactly as Claude's
`--setting-sources` is. And **the
value must be absolute**: a relative one made the CLI resolve it against its own cwd and die on
`EEXIST: mkdir '..\pC\opencode'`, which names neither Schemaic nor the variable. `private_dir`
returns an absolute path, so that is a property of the source rather than something enforced
here. The doubled name in `<root>/opencode/opencode.json` is not a slip either: `XDG_CONFIG_HOME`
names the *parent* the CLI looks for its own subdirectory in.
**The directory is reused, not per-session, and that is measured rather than tidy.** A config
directory OpenCode has not seen makes it bootstrap a plugin runtime into it — a real `npm
install`, writing `package.json`, `package-lock.json` and `node_modules/`. On a fresh directory
it ran **past three minutes printing not one line** before it was killed, which is
indistinguishable from a hang; `--pure` brought the same first turn to **four seconds**. Both
mitigations are kept because they answer different halves: `--pure` (in `harness::turn_args`)
skips the install outright, and reusing one directory pays whatever bootstrap does happen at most
once per **running app** rather than once per session. The consequence is the reason the endpoint is not
in this file — a reused directory **outlives the session**, so the config carries only the
*path* of the per-session endpoint file (`ai::write_endpoint_file`, `O_EXCL`, owner-only, swept
with the others) and the credential never enters the reused directory at all. The `opencode.json`
itself is written plainly rather than through `create_private_new`: it holds no secret, and it is
rewritten every session, so an existing path is the normal case and not the collision `O_EXCL`
refuses.
**But the reuse stops at the instance boundary, and it did not — which was a bug rather than a
trade.** The root was `persist::private_dir(kind)` with no per-instance component, and the file
inside it carries exactly the two things that differ per connection: the path of the endpoint
file, and the tool names this connection's access level offers. OpenCode is a process *per turn*,
so a second window's session re-pointed the first window's **next question** at its own database,
with its credentials and its access level, while the first window's panel, transcript and deltas
all named the first connection. `instance_root(kind)` is
`persist::private_dir(kind)?.join(instance_tag())` with `instance_tag()` = `pid-<pid>`, and both
`write` and `write_inline` go through it — and so do `app/copilot.rs`'s two homes and
`app/cursor.rs`'s two workspaces, which is why
`instance_root`, `instance_path_in` and `instance_tag` are `pub(crate)`: a `COPILOT_HOME` carries
a per-connection path too and is read by a process per turn, so it needs the same boundary for
the same two reasons, and so does a Cursor workspace's `mcp.json` and permissions file.
One correction worth keeping, because the first telling
of this got it wrong: the file was under `persist::private_dir` throughout, so it was per-*user*
and never machine-wide, which is why the fix is a per-instance subdirectory rather than a move
out of a shared location. The pid is the discriminator because that is what a running instance has and a
dead one does not, and the reuse the paragraph above argues for survives: a directory per
instance is still one the CLI has usually seen before within that instance's life.
**`sweep` collects the roots of instances that are gone**, at startup, on the same detached thread
as `antigravity::sweep` (both shell out or walk the disk, and neither is wanted on the UI thread —
see `main.rs`). It is tidiness rather than a permission being withdrawn, because nothing under
these roots is a secret, but a per-instance directory nothing collects is a directory per launch,
forever. It asks `liveness::process_start(pid)` for **any** live process on that pid rather than
for ours specifically, since a second window's root must survive and the cost of being wrong the
other way is one directory kept until the next launch; it also removes the pre-instance
`opencode/` directory an older build left directly under the base. And it removes
**recursively**, unlike `ai`'s session working directories, which is stated rather than assumed:
everything under a root here was written by Schemaic or by an OpenCode bootstrap into a directory
Schemaic made for it, and nothing here is a credential. The walk itself is
`sweep_instances(kinds)`; `sweep` is that over `opencode` and `opencode-inline`, and
`copilot::sweep` calls it over its own two kinds, the recursive-removal argument carrying over
unchanged because what is under a Copilot home was likewise written by Schemaic or by the CLI into
a directory Schemaic made for it. `cursor::sweep` calls it over `cursor` and `cursor-inline` on
the same argument — only after its own walk of the user's Cursor directory, described there.
(The legacy `opencode/` clean-up runs for every kind it is
given; under a Copilot or Cursor base there is no such directory to find.)
**`write` returning `None` must refuse the session, and `ai::start_ai_session` does.** This was
the first harness where a missing config does not fail closed (Cursor is the second, see
`cursor.rs`): `--agent schemaic` naming an agent that
does not exist leaves the run on OpenCode's own `build` agent, which has `bash`. So there is no
degraded mode here of the kind the Codex path takes with `codex_isolation_only` — the user gets a
message saying the assistant is disabled because the file that restricts it could not be written.
Most of the tests here are over the pure JSON (`harness::opencode_config_json`) rather than the
IO. The two that are not pin a *path* and an *environment*:
`no_two_configs_that_must_differ_can_land_on_one_path` calls the real `instance_root` and returns
early rather than asserting when this machine has no config directory, and
`nothing_the_seal_depends_on_is_cleared_by_accident` builds an `OpenCodeConfig` over a literal
root and touches no disk at all. The one worth knowing
is still `the_agent_the_config_defines_is_the_one_the_argv_selects`, which walks
`--agent`'s value out of `turn_args` and back into the document: both sides read `OPENCODE_AGENT`
so a rename can never break the lookup, but the JSON growing a different shape around the name
can, and that failure is silent.
