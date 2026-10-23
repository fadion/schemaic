# `ai/harness.rs`

`harness.rs` — which agent CLI is being driven, what it can do, and how it is constrained.
`Harness` (`Claude`/`Codex`/`Antigravity`/`OpenCode`/`Copilot`/`Cursor`) is **a dialect
rather than a vendor**: the
variant names the wire format a binary speaks, which is the only thing decoding needs to know,
so a fork that still speaks its parent's JSONL is that parent here. `key`/`label`/`bin` are the
value `UiState::ai_harness` persists, the settings label and the name looked for on `PATH` —
three separate strings on purpose, since Antigravity's executable is `agy` and a detector keyed
on the label finds nothing (`each_harness_looks_for_its_own_binary_name`); `from_key` returns an
`Option` and
deliberately does **not** coerce an unknown key to a working harness, because a persisted key
this build no longer knows has to be *reported* and a silent substitution is invisible by
construction. **The reporting is now somebody's job rather than an intention**: `persist` keeps
the raw string (see `UiState::ai_harness`) and `main.rs` is where the `None` is answered — a
`tracing::warn!` naming the value, then the assistant **off**. It used to be Claude, which spawned
a CLI the file did not name for a user who had picked something else; off sends nothing anywhere,
the panel says so, and the dropdown is one pick from whichever agent was meant. The key
round-trips **verbatim**, never mapped onto the known set at the persistence layer, which is precisely what leaves the app something to
report (`an_unknown_harness_survives_the_round_trip_rather_than_being_corrected`); and a
settings file written before the field existed carries no key at all and loads as Claude,
because Claude was the only harness there was
(`a_ui_state_from_before_multiple_harnesses_loads_as_claude`) — while a new install, which has
no file at all, starts off (`persist.rs`'s entry has both defaults).
**`Agent` is the setting; `Harness` is what it drives.** `enum Agent { Disabled, Cli(Harness) }`
sits beside `Harness` rather than adding an off variant to it, because every `match` over
`Harness` — the argv, the stream decoder, the constraint grade — answers a question only a CLI
has, and an off arm in each would be a dozen places to answer "nothing" and one place to forget
to. `Agent::ALL` is the dropdown's order, *Disabled* first and then `Harness::ALL`; `key` is
`Harness::key` or `persist::AI_DISABLED_KEY`, which must never become a harness key
(`the_disabled_key_is_no_harness_key`); and `from_key` returns `None` for an unknown key for
`Harness::from_key`'s reason. The app holds the choice as **two signals**, `ai_enabled` and the
`ai_harness` it already had — off still holds a harness (Claude, when it loaded off), so
everything that takes a `Harness` goes on taking one — and **`Agent::of(enabled, harness)` is
the one place they are composed**, by the dropdown and by the save alike, so the two cannot
disagree about what "off, with Codex held" means: it means off
(`an_off_assistant_is_disabled_whatever_harness_it_holds`).
**Serde kept the key and the app then overwrote it, so the promise held for exactly one
session.** The save site wrote `ai_harness.get().key()` — the *fallback's* key — so the name
the user's file carried was replaced by the next thing that touched the settings, and a build
that later grew that harness had nothing to restore. The rule is
`persist::ai_harness_to_persist(unknown, running)`: `main.rs` holds the raw key in
`ai_harness_unknown` (the modal's `AiUi::unknown_key`) and writes it back
until the user picks, at which point it is cleared and the file starts naming what runs.
**Whether a pick clears it is `Agent::apply(enabled, harness)`'s answer**, the inverse of
`Agent::of` and pure in `schemaic-ai`: it drops the key when the pick changes `enabled` *or*
the harness. It used to be two effects, the harness-switch effect and a second one on
`ai_enabled`, and the bug sat between them: an unknown key loads off *holding Claude*, so
picking *Claude Code* changes `ai_enabled` and not `ai_harness`, and with only the first effect
the save went on writing the unknown key back and the next launch started off again
(`picking_the_held_cli_from_an_unknown_key_turns_on_and_drops_the_key`). Picking *Disabled*
while already off changes neither, so the key is still kept
(`picking_disabled_while_an_unknown_key_is_held_keeps_the_key`). **The same load also clears `ai_model` and
`ai_cli_path`**, which were chosen for a CLI this build does not have: the switch effect clears
those on a *switch* and a restore is not one, so an unknown harness paired with
`opencode/claude-sonnet-5` started a Claude session with that model id and died on an unknown
model under "check your installation". The test above was green throughout, because it drove
serde in isolation and never the composition with the caller that overwrote; it calls the rule
now, which is the only reason it can fail.
**`speaker_name` is a fourth string, and it differs from `label` in exactly two places on
purpose**: `label` is "Claude Code", the product you install and point a path at, while a
transcript header is naming a speaker and reads "Claude" — which is also the name that header
carried before it learned to vary, so an existing conversation does not appear to change its
mind about who wrote it. The second is Copilot's: the settings box names the product a user
installs, "GitHub Copilot", while "GITHUB COPILOT" over every answer reads as a heading rather
than a speaker, so the header says "COPILOT". The other four answer the same in both, spelled
out rather than delegated to `label` so that a product name gaining a suffix cannot reach the
transcript on its own (`the_speaker_name_is_the_product_name_except_where_it_is_deliberately_not`
pins both divergences and the agreement of the rest). The free function `speaker_label(Option<&str>)` is the only thing that turns a
persisted `ChatMessage::harness` back into a name: the uppercased speaker name, or `"ASSISTANT"`
for `None` **and** for a key this build does not recognise. Both fall back rather than guess, for
`from_key`'s reason — a transcript written before the field existed could have been produced by
Claude, Codex or Antigravity, the three `Harness::ALL` held when the field was added alongside
`OpenCode`, and nothing on disk says which; one written by a later build may name a harness this
one has never heard of. Naming Claude there would be the `AiModel::from_cli` failure put back at
the one place the user can read it. Everything else is asked as a
**capability** and never as a variant
(`supports_effort`, `effort_levels`, `effort_arg`, `supports_resume`, `is_persistent`,
`session_turn_line`, `session_interrupt`, `session_system_in_first_turn`,
`streams_deltas`, `supports_model_choice`, `suggested_models`, `restricted_means_sandbox`,
`seals_by_flag`, `restricts_by_workspace_rules`, `isolates_mcp_servers`, `help_args`) — the
same reason the engine
predicates exist:
`== Harness::Claude` compiles cleanly while sorting the next CLI onto whichever side it happens
to fall. **There is no exception left.** `Constraint::notice` was documented here as the one
deliberate `h == Harness::Claude`, and OpenCode is exactly the fourth CLI that shape sorts wrong;
it is now three capabilities, described below, and there is no `== Harness::` left in that
`match` — the comment paragraph that used to defend the identity check as "a harness question"
went with it, since it stood next to the paragraph explaining why it was not.
**And every capability is an exhaustive `match`, not a `matches!`**, for the reason `env_seal`'s
own doc gives ten lines from three of them: a fifth harness is then a compile error here rather
than a silent default there. Five were still `matches!` beside five that were not, and what the
default would have cost is written on each — `supports_model_choice` answering `false` hides the
model field for the new CLI entirely, which is the failure `suggested_models`' doc records one
step over; `session_system_in_first_turn` gives a persistent fifth harness no schema outline on
its first turn, so it answers about a catalogue it was never shown; `seals_by_flag` routes
`Constraint::Restricted.notice` to the arm written "to promise nothing rather than to be
unreachable, because 'unreachable' is what the arm above assumed too"; and
`is_persistent`/`streams_deltas` simply answer wrongly and quietly. No behaviour changed and
nothing could be watched failing — every arm answers what the `matches!` answered, and what it
buys is the compile error. `supports_model_choice` reads oddly as a result — one arm, every
variant, `true` — so its doc says why it is still a `match`.
**`suggested_models` is a menu, never a permitted set**, and the distinction is the whole reason
it returns a slice rather than the field being a dropdown: the model id is a free string, so a
list here is the aliases each CLI documents as stable and anything dated, private or newer than
this build is typed in and passed through untouched
(`suggestions_are_a_menu_and_never_a_permitted_set`,
`a_model_id_no_build_has_heard_of_still_reaches_the_command_line`). Antigravity's list is
deliberately **empty**, because its model names were never measured — and an empty list means
"nothing useful to suggest, let them type", not "no models allowed", so a caller must render the
field regardless. OpenCode's entries are **`provider/model`, and the provider half is not
decoration**: a bare `claude-sonnet-5` is not an id that CLI accepts, and every line
`opencode models` prints is qualified. The four suggested name the built-in `opencode` provider,
which is the one an install has before the user adds credentials of their own; somebody
authenticated straight to a vendor types `anthropic/…` or `openai/…` and the free-text field
passes it through untouched. That is the failure this API is shaped against: the app shipped a closed
three-variant `AiModel` for a year, and its `from_cli` coerced every unrecognised id to Haiku, so
a settings file naming a real model ran a different one with nothing on screen to say so.
Copilot's list is **`auto` alone, because its catalogue is the account's**: on the account it was
measured with (1.0.88, 2026-09-26), every named id — the examples `copilot help config` itself
lists included — was refused with *"Model "…" from --model flag is not available"*, and only the
router ran. A named chip is a chip that fails the turn for somebody; `auto` is the one id every
plan takes. Cursor's is `auto` alone too, for a milder reason: `agent models` lists the
account's own catalogue, long and plan-specific, and `auto` is the default every account has.
**A capability that is a `bool` can still be too coarse, and `supports_effort` is where that
showed.** It is true for Claude *and* Antigravity, so a site asking only it treated the two as
interchangeable — and their `--effort` vocabularies are not: Claude takes a fourth level,
`xhigh`, where `agy`'s flag documents `low|medium|high`. Effort is kept across a harness switch
rather than cleared as the path and the model are, so choosing `xhigh` under Claude and
switching sent `agy --effort xhigh`: one CLI's vocabulary applied to another, the
closed-model-list bug in a second dress. `effort_levels` is the list and `supports_effort` now
*computes* itself from `!is_empty()` so the two cannot disagree, while the argv sites ask
`effort_arg(requested)`, which answers `Option<&'static str>` **taken from that harness's own
list** rather than echoing the caller's string — so what is sent cannot be a level the harness
never advertised, whatever is passed in
(`a_level_one_harness_takes_is_not_sent_to_another_that_does_not`,
`each_harness_offers_only_the_effort_levels_its_own_flag_takes`). The settings modal clamps the
*selection* as well (`AiEffort::clamped_to`, under `settings.rs`), but that is about what the
box shows; this is the one that decides the command line.
**OpenCode made the list non-contiguous, which is why it is a list and not a ceiling.** Its flag
is `--variant` and the three levels its help names are `minimal|high|max` — no `low`, no
`medium`, and a `max` above Claude's `xhigh`. The list is deliberately *not* widened to the union
with anyone else's: that help text calls the accepted set provider-specific, so it is not even
constant across OpenCode's own models, and `effort_arg` clamping to what the flag documents sends
no flag rather than an invented level when `xhigh` or `medium` arrives from another harness
(`another_harness_effort_level_is_clamped_before_it_reaches_argv`). **That test used to apply
`effort_arg` in its own body and pass the answer in**, which is the caller's line copied into
the test: it was green whether or not any caller clamped, and `turn_args(OpenCode, effort:
"xhigh")` really did return `--variant xhigh` at the time. It hands the argv builder the *raw*
level now and loops every harness × every level — see the clamp's move into the builders,
below, and the gap widening it found on Claude. The knock-on is in
`schemaic-ui`: a four-variant `AiEffort` intersected this list at exactly `High`, so see the
settings-modal entry for why the enum is now the six-way union.
**Copilot has the flag and its list is empty, which is the one place the two part company.**
`--reasoning-effort` is real, but with no `--model` Copilot routes through `auto`, and `auto`
refuses it outright — *"Model "auto" does not support reasoning effort configuration (requested:
"high")"*, exit 1, no answer. Whether a *named* model takes it could not be measured, since no
named model was available on the measuring account. A control that kills the turn on the default
setting is worse than none, so `effort_levels` is empty, the row hides, and `effort_arg` can never
produce a level to send (`effort_reaches_each_harness_in_its_own_flag` asserts no
`--reasoning-effort` reaches the one-shot argv; `turn_args` has no arm that could emit one).
Cursor's list is empty for the plainer reason that it has **no flag at all**: effort is part of
the model id there, a named variant (`gpt-5.3-codex-high`) or a bracket parameter
(`…[effort=high]`), both of which the free model field passes through untouched — so the row
hides and the same test asserts nothing naming effort reaches its argv.
**The seal is graded here, because the six do not answer it equally well.** `Constraint` is
ordered `Unknown < Restricted < Sealed`, and **three** harnesses reach the top by three different
mechanisms. Claude does it with a flag: `--tools ""` empties the *built-in* set outright and
leaves nothing but the allow-listed MCP tools. OpenCode does it with *configuration* — the agent
definition `opencode_config_json` writes carries a `tools` map setting every documented built-in
to `false`, and `opencode debug agent schemaic` then resolves that agent to eleven disabled
tools and one enabled — `invalid`, the CLI's own handler for a malformed tool call, which its
own description calls *"Do not use"* and which it excludes from the set it offers the model
(`activeTools: …filter(m => m !== "invalid")`). The twelve written and the eleven returned are
not the same eleven: `lsp` and `websearch` are documented by `--permissions` but are not
registered tools in the measured build and drop out, while `question` is never written here and
comes back disabled anyway. The map is a **denylist by omission**, which is the load-bearing
detail: a name absent from it stays *enabled*, so it has to name every tool the CLI documents,
and one a future build adds is live until somebody adds it here. That asymmetry is why the list
is deliberately over-inclusive: a name that has gone costs nothing, a name that is
missing costs the seal. `the_agent_has_every_built_in_tool_switched_off` (under `app/opencode.rs`)
asserts the map is non-empty and that nothing in it is left on, and names `bash`/`read`/`write`/
`edit` explicitly, because "everything in the map is false" stays true if a rename drops them out
of it. Copilot does it with an **allowlist of what the model is shown** — `--available-tools`,
*"Only these tools will be available to the model"*. Measured on 1.0.88 (2026-09-26): given
`schemaic-list_schema,schemaic-run_query`, the tool list the CLI sent the model — which its own
`session.usage_checkpoint` reports — was exactly those two, `session.info` listed every built-in
as disabled, and a turn told to read a file "using any tool you have" answered that it could not
and named only ours. That is `Sealed` in the sense `--tools ""` is: nothing built-in left,
rather than a sandbox around what is. The allowlist is the connection's own, the same
`ai_allowed_tools` Claude's `--allowedTools` gets, carried per turn on `TurnSpec::allowed_tools`
because this seal lives on a command line that is rebuilt for every turn, and spelt the way
Copilot names an MCP tool, `<server>-<tool>`, through `bare_tool_name` — so a schema-only
connection's model never *sees* `run_query`, which is visibility and not merely approval
(`the_allowlist_names_our_tools_the_way_copilot_does_and_follows_the_access_level`).
**The flag fails open when it is empty, and that is the load-bearing detail here.** Measured:
`--available-tools` with no value, or with `=` and nothing after it, sent the model all 24 of the
CLI's tools, `powershell`, `create`, `edit` and `web_fetch` among them — so the list would have
been widest exactly where it was meant to be tightest, on an access level offering nothing and on
every one-shot. `copilot_available_tools` therefore never emits an empty list: an empty one
becomes `COPILOT_NO_TOOLS` (`schemaic-no_tools`), a name in our own server's namespace that
nothing provides, and that allowlist was measured to resolve to **zero** tools. The one-shot path
always passes it. `an_empty_allowlist_is_never_an_empty_flag` pins the constant and both argv
builders, and asserts no bare `--available-tools` appears on either — "tidying" the sentinel
into an empty value is the edit that opens the shell.
Codex and Antigravity are both `Restricted`, each by its own lever: Codex by
`sandbox_mode="read-only"`, Antigravity by `--sandbox`. Both
stop writes and command side effects and both leave built-in tools that can still *read*
the machine the child was launched on — and Antigravity is the one where that is measured rather
than inferred, since a print-mode session lists **56** built-in tools with `--sandbox` passed
(`run_command`, `write_to_file` and `execute_browser_javascript` among them), gated by nothing
but a permission mode, and a measured turn ran `list_dir` and `view_file` with no prompt at all.
There is no flag that empties that set, which is why the grade is a ceiling and not a
to-do. So `Constraint::notice` states that to the user in one line, and
returns `None` for `Sealed` — a banner on every sealed session is how a user learns to ignore
the two grades that mean something.
**Both keep a shell, too, and a shell can run `schemaic` itself** (security review S2): Codex's
under its read-only sandbox, Antigravity's as `run_command`. `schemaic exec` is a write the
panel's read-only MCP tools never offer, and `query`, `export`, `dump` and `mcp` read past the
panel's AI data level, on any connection the user has granted CLI access — provided the vendor's
sandbox lets the child run it and reach the database's socket and the OS keyring. Measured on
Windows 11 (2026-10-23), each harness spawned with the panel's own argv and environment: Codex CLI
0.153.4 rejected **every** shell call as *"blocked by policy"* before a process started — not only
`schemaic list`, a TCP connect and a keyring read but `whoami` and a `Get-Content` in its own cwd —
and Antigravity 1.4.6's headless mode auto-denied `run_command` for every command, ending the turn, its
`schedule` tool's work included when that re-entered `run_command`. The same probe outside a
harness reached all three database ports and the keyring. So on Windows neither shell reaches
`schemaic` at all. Antigravity's notebook and browser tools were *reported* unavailable in this
mode by the model, which made no call, so that is not proven; and Linux and macOS are unmeasured
— there Codex's Landlock and Seatbelt sandboxes let a command run and restrict its network and
writes instead, so the answer may differ. All three agent spawns in `app/ai.rs` — `run_inline`, the
session spawn and Antigravity's `spawn_child` — set `schemaic_core::AGENT_CHILD_ENV`
(`SCHEMAIC_AGENT_CHILD`) to `1`, which anything the agent's shell runs inherits, and the command
line's `dispatch` refuses every command but `version` under it with `Exit::Refused` (under
`cli/run.rs`). **It is a speed bump, not a boundary.** A shell that knows the name can unset it,
so what stands between the agent and the keyring is still the vendor's sandbox, and the
plaintext `connections.json` of the keyring-unavailable fallback stays readable by the built-in
file readers above, which no environment variable reaches — the same run's `view_file` read a file
outside its cwd without a prompt. The panel's own MCP subprocess is
unaffected: `--mcp-serve` is answered in `app/main.rs` before the command line's dispatch, so it
never asks. **That made it a route of its own** (release review S3-L5-03): an agent with a shell
could start `<exe> --mcp-serve` with an endpoint of its own making and serve *any* saved
connection by id, `cli_access` never asked. So the panel's spawn writes a **grant** —
`schemaic-mcp-<owner>-grant-<tag>.json`, holding the connection id, in the owner-only `ai-mcp`
directory beside the session's other private files and removed with them — and puts its bare
name in the endpoint blob as `"grant"`; `mcp_endpoint_from_env` asks `grant_allows` before it
builds anything, and serves a saved connection only when the grant names the same one. A blob
naming no saved connection needs none. An agent that reads its MCP config learns the grant and
with it reaches the connection the panel already gave it, at that connection's own AI data level
(`samples` is bounded by it, S2-L5-03) — nothing else; writing a grant of its own would need a
write into the app's private directory, which is the vendor sandbox's to stop
(`mcp_serve_serves_only_the_connection_the_app_granted`, which also pins the order in
`mcp_endpoint_from_env` from the source). Verified on the built binary: a forged blob for a saved
connection, and one whose grant names another, exit 2 with the refusal; the matching grant serves
the tools; and the Claude harness in a sandboxed app answered a `list_schema` question through it,
the grant gone from `ai-mcp` once the app closed.
**Cursor is the third `Restricted`, and it is reached by neither a sandbox nor a flag but by rules
in a file** — the weakest mechanism here, which is why the grade and the notice both have to say
so. Measured on its CLI (2026.09.23-86fc751, Windows 11, 2026-09-26): nothing empties its
built-in tools, it has a `--sandbox` flag that is not what restricts it, and its `--mode ask` is
**not enforced** — told the Write tool still worked, the model wrote the file. What does hold is
a project permissions file, `<cwd>/.cursor/cli.json`, whose `deny` names `Write(**)`,
`Shell(*)` and `WebFetch(*)` (`CURSOR_DENY`): those refused writes, deletes and commands, a
`Task` subagent's included, and beat an `allow` of `Shell(ls)` in the user's own global
`cli-config.json`. `Read(**)` and `Grep(**)` are denied too and are **not** a seal — the `Read`
tool was refused and `Grep` still returned a file's contents — so the readers cannot be closed
and the grade is a ceiling, never `Sealed`. `cursor_permissions_json(allowed)` builds the file
from the connection's own list, each tool an allow rule in the CLI's own form,
`Mcp(schemaic:<tool>)`, and **allowing is required rather than merely permitted**: an MCP call
with no allow rule is refused headless (*"User rejected MCP"*), so a schema-only connection's
model can see `run_query` in the catalogue and is refused when it calls it. An empty `allowed`
is an empty allow list, which is safe here where Copilot's empty allowlist was not: Cursor's
allow rules only approve, so nothing opens when there are none
(`the_rules_allow_exactly_the_connection_s_tools`,
`the_rules_deny_the_writers_whatever_is_allowed`). **And a missing file fails open** — with no
permissions file at all a headless turn refused its shell and every MCP call, and *wrote a file
unprompted* — which is why the refusal on a failed write, below, is part of the grade rather
than tidiness. `--mode ask` is passed anyway, as a prompt that makes the model rarely try, and
never `--approve-mcps`, `--force`, `--yolo` or `--auto-review`, which approve what the file
exists to refuse (`nothing_that_approves_what_the_rules_refuse_is_ever_passed`).
**A notice says what *this* harness gives you and stops there.** `Restricted` reads
*"`<Harness>` runs sandboxed: its own built-in tools cannot write files or run commands, though
they can still read this machine"*, and it used to close with *"Only Claude Code can be given no
built-in tools at all"* — a statement about the grade turned into an advertisement for the
harness the user had just declined, printed in the one place they were exercising the choice.
`the_weaker_grades_say_so_and_the_strongest_stays_quiet` pins both halves, the second over every
harness × every non-`Sealed` grade: a notice may name Claude only when the harness *is* Claude.
**And `Restricted` does not mean one thing, so the sentence has to describe the mechanism.** For
Codex and Antigravity the grade is a real sandbox and *"cannot write files or run commands"* is a
claim the OS is enforcing. For Claude it means only that `--help` did not list `--tools`, so the
seal fell back to `DISALLOWED_TOOLS` — the denylist this section opens by describing as the guard
that looked total and left nineteen built-ins live, several of which ran with no permission request
at all. Printing the sandbox sentence there would be the comfortable lie one grade down, shown to
the user as a positive assurance, so Claude gets its own line naming the denylist and pointing at
a CLI update (`a_denylisted_claude_is_not_described_as_unable_to_write_or_run`).
**That was asked as `h == Harness::Claude`, with everything else falling through to the sandbox
wording, and it is the harness-identity check this document warns about wearing the name of a
justification.** OpenCode is the fourth CLI it sorts onto the wrong side: its seal is a `tools` map
and it has no sandbox at all, so the fallthrough would have promised an OS-enforced read-only that
nothing enforces — the strongest kind of false claim, made to the user as a positive assurance. The
question is now four capabilities. `restricted_means_sandbox` (Codex, Antigravity) picks the
sandbox sentence. `seals_by_flag` (Claude alone) picks the denylist line, because *"updating the
CLI restores the full seal"* is advice that fixes nothing where the seal is configuration —
OpenCode reaches `Sealed` too, and by a file. **Copilot seals by a flag and still answers
`false`**, because the question is about the *weaker* grade: without `--available-tools` there is
no denylist to fall back on, so that binary is `Unknown`, never `Restricted`, and never reaches
the sentence the predicate chooses. `restricts_by_workspace_rules` (Cursor alone) picks a
sentence of its own — *"`<Harness>` is kept to answering by a permissions file Schemaic writes
for this session, not by a sandbox: its own tools are refused writing files and running
commands, but they can still read files on this machine"* — and it names the mechanism for the
reason this paragraph exists: Cursor answered `false` to both predicates above, so without it
Cursor would have taken the promise-nothing arm, and giving it the sandbox arm instead would have
promised an OS boundary nothing provides. The readers are named because they are the half no
rule closed. A last arm catches a harness that is none of these and **promises nothing**,
saying only that Schemaic could not fully restrict it. No harness
reaches that arm today, and it is worded rather than left unreachable precisely because
"unreachable" is what the Claude arm assumed too.
**The sandbox covers the CLI's own tools, and the sentence used to claim more than that.** It read
*"it cannot write files or run commands"* flat, while `harness.rs`'s own header records that MCP
tools execute *under* `--sandbox` and that one harness has no MCP isolation at all — a positive
assurance the module contradicts eleven paragraphs above it, which is the asymmetry the grade
exists to prevent. The claim is now scoped to *"its own built-in tools"*, and the fourth capability
is `isolates_mcp_servers`: true for Claude (`--strict-mcp-config`), Codex (the whole `mcp_servers`
table assigned out from under it), OpenCode (`XDG_CONFIG_HOME` redirection) and Copilot (its own
`COPILOT_HOME`, with `--disable-builtin-mcps` and the allowlist behind it), false for
Antigravity and Cursor, measured rather than assumed. When it is false the notice says so, in
words each harness's measurement earned: Antigravity's sandbox sentence appends *"Any MCP servers
you have registered with it are also available to this session, and Schemaic cannot restrict
them"*, and Cursor's rules sentence *"MCP servers you have registered with it also start with this
session; their tools are refused unless your own settings allow them"* — different facts, since
Cursor's permissions file does refuse those tools where Antigravity's rules name only ours. So the
user-global gap this document records above and again under `app/antigravity.rs` and
`app/cursor.rs` is disclosed in the panel rather than only in this file.
`a_harness_that_lets_other_mcp_servers_in_says_so` keys on the phrase the two wordings share,
*"MCP servers you have registered"*, rather than on either's second half — it keyed on
*"Schemaic cannot restrict them"* until Cursor's wording made that Antigravity's alone — and
spells out which harnesses are isolated rather than reading it back off the notice.
**The two tests that used to pin all this asserted the notice against the predicate it branches
on, and that is why the replacement spells its table out by hand.**
`only_a_sandboxed_harness_is_told_the_os_is_stopping_it` computed
`notice.contains("cannot write files or run commands") == h.restricted_means_sandbox()` — the
substring compared to the very predicate that produced it — so flipping `restricted_means_sandbox`
to include OpenCode, the exact regression the capability split was written for, left the suite
green, and nothing pinned OpenCode's `Restricted` notice at all.
`each_harness_is_told_what_is_actually_restricting_it` replaces both it and
`only_a_flag_sealed_harness_is_told_an_update_would_help`, spelling out a
`(sandbox, update, other-servers)` triple **per harness** in a `match`: Antigravity
`(true, false, true)`, Codex `(true, false, false)`, Claude `(false, true, false)`, OpenCode
and Copilot `(false, false, false)`, Cursor `(false, false, true)`. A change to any of the
predicates now has to be argued for in that table. It also asserts that a notice says
*"permissions file"* and *"not by a sandbox"* exactly when `restricts_by_workspace_rules` is true,
and that such a notice says its tools *"can still read files"* — the mechanism named, and the
half it does not close. That half compares the notice with the predicate that produced it, the
shape this paragraph faults; what keeps it from being circular is `cursor_tests`'
`the_capability_answers_match_what_was_measured`, which pins the predicate true for Cursor and
false for every other harness by hand.
**`Constraint::Unknown` refuses the spawn** (`is_runnable` is false), which is the opposite
failure direction from `CliSeal`/`seal_from_help` above, and both are right for the same
underlying fact: an unknown flag *kills* the child. An unreadable Claude probe therefore yields
`CliSeal::ALL`, since the worst case there is a loud death rather than a quiet unsealed session —
but passing `--sandbox` to a binary that never heard of it *is* that loud death, so for a
non-Claude harness the conservative answer is refusal rather than assumption
(`an_unreadable_probe_refuses_rather_than_assuming`,
`a_harness_missing_its_constraining_flag_is_not_runnable`,
`an_unreadable_probe_refuses_every_harness`,
`each_harness_reaches_exactly_the_grade_its_mechanism_earns` — the last two run over
`Harness::ALL`, replacing an `only_claude_can_reach_the_sealed_grade` that asserted three
harnesses one at a time and so held its own name for none of them).
**A seal that is not a flag needs a different probe, and OpenCode's arm greps `--pure`.** Every
other grade is read straight off the lever itself: `--tools`, `--sandbox`, either present in the
help text or not. OpenCode's lever is a file Schemaic writes and an environment variable it sets,
and `--help` can confirm neither. `--pure` is what it greps instead — greppable, passed on every
turn, and *genuinely part of the constraint*, since it is what keeps the user's external plugins
(arbitrary code they installed) out of the session. It stands as evidence that the binary is the
OpenCode these mechanisms were measured against; a help text without it is `Unknown` and refuses,
exactly as with the others (`the_constraint_is_sealed_only_when_the_probe_read_a_help_page`,
whose second half holds a readable help page for something that is *not* OpenCode to `Unknown`
rather than to a grade). Copilot's arm is back on the lever itself — `--available-tools` present
is `Sealed`, absent is `Unknown` — and, unlike Claude's, has **no `Restricted` fallback**, since
without the flag there is no denylist to fall back on (`the_grade_is_read_off_the_flag_the_seal_is`,
whose fixture rows are copied from `copilot --help` 1.0.88). Its top-level page lists every flag
the grade and the argv depend on, so `help_args` asks it no subcommand. **Cursor's arm is
OpenCode's shape again, and greps `--trust`**: its lever is a file too, which `--help` cannot
confirm, so the evidence is a flag passed on every turn that the rest of the mechanism was
measured alongside — without it a headless run in a directory the CLI has not seen stops at a
trust prompt and exits. Present is `Restricted`, absent is `Unknown`, and there is no arm that
reaches `Sealed` (`the_grade_is_restricted_at_best_and_read_off_trust`, fixture rows copied from
`agent --help` 2026.09.23).
**The other half of that grade is enforced at the call site, because this is a harness where
a missing config does not fail closed.** `--agent schemaic` naming an agent that does not exist
does not stop the run — it leaves it on OpenCode's own default `build` agent, which has `bash`.
So `ai::start_ai_session` **refuses the session outright** when `OpenCodeConfig::write` returns
`None`, rather than degrading the way the Codex path does with `codex_isolation_only`. A seal that
silently becomes a shell is precisely what `Constraint` exists to prevent, and here the grade is
only honest because the refusal is there. (Copilot's session is refused on a failed write too, but
for its *isolation* rather than its seal — the allowlist is on the command line and holds without
the home; see `app/copilot.rs`.) **Cursor is the second harness this is true of, and more
literally**: its file *is* the restriction, and without it writes go through unprompted, so a
session whose workspace cannot be written is refused, and so is a one-shot (`ai::inline_cwd`).
The grade's own comment says the same, since `--help` can confirm nothing about a file.
**`env_seal` is the capability both of those ride on, and it is asked, never assumed.** It answers
whether a harness's configuration is an *environment* Schemaic must set on the child and clear
the user's copy of: OpenCode (`XDG_CONFIG_HOME`, which carries its seal) and Copilot
(`COPILOT_HOME`, which carries its isolation — its seal is the flag). It exists because, by its own
doc, four call sites used to ask the question four ways, none compile-forced — a `match {
OpenCode => …, _ => Vec::new() }` for the inline environment, an `== Harness::OpenCode` on the
session spawn, an `== Harness::Codex` in the probe — each failing to the unsafe side for a new
harness. **Copilot is the case that showed asking it is not enough on its own**: the
one-shot path asked `env_seal()` and then called `OpenCodeConfig::write_inline()` for any `true`,
so a second `true` would have been "isolated" by OpenCode's config and nothing of its own.
`ai::inline_env` now asks `env_seal`, then matches the harness exhaustively — see `app/ai.rs`.
**Cursor answers `false`, although its restriction is a directory too**, because it finds that
directory by being *started* in it — its working directory — rather than by a variable pointing
at it; an `env_seal` of `true` would route it through `inline_env`, whose arm for it refuses. Its
counterpart is `restricts_by_workspace_rules`, and the one-shot's counterpart of `inline_env` is
`ai::inline_cwd`, built the same way: the capability asked, then an exhaustive `match` whose arms
for the harnesses it is false for refuse.
**Codex's isolation is conditional and does not decide the grade.** `--ignore-user-config` is
that CLI's `--strict-mcp-config` and `--setting-sources user` rolled into one — its own help text
says *"Do not load `$CODEX_HOME/config.toml`; auth still uses `CODEX_HOME`"*, so it drops the
user's MCP servers and their hooks while keeping them logged in. It is passed only when the
probe saw it (`codex_isolates_config`), exactly as Claude's two isolating flags are, and like
Claude's it leaves the grade to `--sandbox`/`--tools` alone
(`isolation_does_not_decide_the_grade`). Note the two probes answer an absent flag in opposite
directions on purpose: a missing isolation flag is a known, lesser, still-runnable state, while
a missing *grade* means nobody knows whether side effects are blocked at all.
**Claude's pair is not total either, and what `--setting-sources user` keeps includes commands**:
the user's own `~/.claude/settings.json` still loads — its `hooks` and `apiKeyHelper`, run with
the child's environment, its `env`, and its plugins, whose skills may bring back what `--tools ""`
removed (a plugin's MCP servers stay out, `--strict-mcp-config` covering them) — so the residual
is a tampered user config, the very hooks Codex's `--ignore-user-config` drops.
**And "the probe saw it" was false on every real machine, because Codex keeps that flag off its
top-level help page.** Measured against the installed binary: `codex --help` lists `--model`,
`--sandbox`, `--help` and `--version` and *not* `--ignore-user-config`, while `codex exec --help`
lists all of them. The probe asked the top-level page, so `codex_isolates_config` answered `false`
always, the flag was never passed, and every Codex session loaded the user's own
`~/.codex/config.toml` — their MCP servers, whose tools nobody here allow-listed, and their hooks,
which are commands. `Harness::help_args` is the fix and is a **capability rather than a literal at
the call site**, because the seam is where the bug lived: the pure decoder was tested with
`codex exec --help` text pasted in by hand while `probe` fed it a different page, so both halves
passed alone and neither was wrong on its own. It answers `["exec", "--help"]` for Codex and
`["--help"]` for the other five, and the last element is always the help flag itself, since every
harness's page still has to answer the grade question
(`codex_is_probed_on_the_subcommand_that_documents_its_isolation`, which pins this function's
*return* — the answer, not the question the probe asks; the argv itself is pinned in
`agent_cli.rs`, and why it had to be is under that entry). `codex exec --help` also
carries `--sandbox`, so one page still answers both — and
`the_codex_help_page_answers_the_grade_and_the_isolation_together` keeps the *old* page as a
fixture beside the new one, holding it to a resolved grade and a silently absent isolation, so the
regression stays legible rather than merely gone.
**The endpoint never reaches argv, and that forced a new mechanism.** Codex's only configuration
lever is `-c key=value`, and those *are* argv — readable by every process listing on the
machine — so the DB endpoint could not travel that way without undoing review C6. The override
names an endpoint *file* instead: `schemaic --mcp-serve --endpoint-file <path>`, where the path
is not the credential and what it points at was, until the endpoint named its saved connection
in place of the password (`app/ai.rs`)
(`the_endpoint_never_reaches_a_codex_command_line` drives a real `mysql://root:hunter2@…`
through it). **Claude is the only harness left carrying the endpoint itself** — it is configured
by a file Schemaic writes *per session*, so the endpoint still travels in that file's `env` map,
in `$SCHEMAIC_MCP_ENDPOINT`, exactly as before. Antigravity takes `--endpoint-file` too, in the
argv `agy mcp add` registers, and OpenCode takes it in the `command` array of the `opencode.json`
written for it — a file, like Claude's, but a **reused** one that outlives the session, which is
why it carries the path and never the blob
(`the_endpoint_path_is_configured_but_the_endpoint_is_not`). Copilot is OpenCode's case again:
`copilot_mcp_config_json` is a file in a `COPILOT_HOME` reused across sessions, so its `args`
carry `--endpoint-file <path>` and it has no `env` map at all
(`the_mcp_config_names_our_server_by_path_and_nothing_else`). Cursor is the same case a third
time: `cursor_mcp_config_json` is the `.cursor/mcp.json` in a workspace reused across sessions,
its `args` carry `--endpoint-file <path>`, and it has no `env` map
(`the_server_is_configured_by_path_and_nothing_else`). So the split is one harness carrying
the endpoint against five carrying a path to it, for two different reasons: argv is public, and
a reused directory is persistent (`main.rs`'s comment beside `--mcp-serve` says the same, and it
named Gemini on the file side until that harness went).
`codex_mcp_overrides` **replaces the whole `mcp_servers` table** (`mcp_servers={schemaic={…}}`)
rather than setting `mcp_servers.schemaic.command`, because `-c` merges: the user's own servers
would otherwise join Schemaic's SQL assistant with nobody here having allow-listed them.
Its values go through a private TOML string escaper, since every separator in
`C:\Users\…\schemaic.exe` is a TOML escape introducer and an unescaped one is either a parse
error or, worse, a different path (`a_windows_path_survives_the_toml_override_intact` walks the
output and asserts no lone backslash survives).
**Something also checks the *structure*, and for a while nothing did.** Five tests asserted
substrings of a string Codex has to parse as TOML, built by a `format!` that is a nest of
doubled braces — so the braces balancing was a property nobody would notice losing until a
session came up with no server and a parse error on a stderr nobody reads.
`the_override_is_structurally_a_toml_value_for_every_access_level` walks the access levels
against three exe shapes (a Unix path, a Windows one, and one with an embedded quote), splits
`key=value`, and checks `{}`/`[]` balance with a small string-aware scanner that is itself
pinned by `the_balance_check_rejects_what_it_is_for`. It is **deliberately not the `toml`
crate**: four transitive dev-dependencies to settle a Low costs more than it buys, and the named
failure — the braces stopping balancing — is exactly what a balance check catches. And it
explicitly does **not** claim Codex accepts the result; only a real `codex exec` settles that,
and the sentences here that say *measured* are where that lives. Registering the server is
**not** enough to make
it callable: measured, with the server registered and no approval set, `codex exec` refuses
every call with *"MCP tool call requires approval, but approval policy is never"* — `exec` has
nobody to prompt, so the default `auto` denies. So the override carries
`tools.<name>.approval_mode = "approve"` per tool, built from the **connection's own**
allow-list, which is what keeps it safe: a schema-only connection approves `list_schema` and
`describe_table` and leaves `run_query` unapproved rather than trusting the server alone to
refuse it — the same rule Claude's `--allowedTools` states. Codex keys that config by the
*bare* name the server advertises while the app's list holds the qualified one, so
`bare_tool_name` derives one from the other rather than a second hand-written list, which would
be one rename away from approving a tool that no longer exists while refusing one that does.
It **delegates to `transcript::ToolCall::bare_name`** rather than re-spelling the expression
under that very paragraph, which is what it used to do: the grant and the chip label the user
reads have to name the same tool, and a change to the `__` separator would have split them with
the grant being the half nobody can see.
(`--ignore-user-config` does not discard these overrides — confirmed against the binary.)
**When the endpoint file cannot be written the override becomes `mcp_servers={}`, not nothing.**
That is `codex_isolation_only`, taken by `ai.rs`'s `(Harness::Codex, None)` arm: the session
loses its database tools either way, but assigning the whole table is the only thing that
displaces the user's own servers, so dropping the override entirely dropped the *isolation* with
it and left every server in `~/.codex/config.toml` loaded into an assistant that had
allow-listed none of them — reachable by the route `codex exec`'s approval policy does not
cover. An empty table is the honest version of "no database tools": ours absent, and nobody
else's in its place (`losing_our_server_does_not_hand_the_session_somebody_elses`, which holds
both overrides to assigning the table rather than to two hand-copied strings).
**Antigravity's two pieces of global state are shaped here and written by `app/antigravity.rs`.**
`antigravity_allow_rules` builds one `mcp(<MCP_SERVER>/<tool>)` rule per tool from the same
connection allow-list, through the same `bare_tool_name` — it interpolated the literal
`schemaic` until the rules and the registration were found to be two independent spellings of
one name, which is the `SERVER` story under `app/antigravity.rs`;
`antigravity_settings_with_rules`,
`_without_rules` and `_with_only_rules` are the `settings.json` surgery, pure so that the part
that can destroy a user's file is the part that is unit-tested. **Merged, never rewritten**: it is the user's file,
it holds their `trustedWorkspaces`, and that CLI rewrites it itself, so the document is parsed,
the rules are unioned in, and every other key is handed back untouched. Adding is idempotent, so
a crashed session's leftovers do not accumulate; removing prunes empty `permissions`/`allow`
containers so the file returns to its prior shape rather than keeping scaffolding, but **only
the ones this call emptied** — the prune used to test the *post-`retain`* state instead, so a
user whose own file already held `{"permissions":{"allow":[]}}` lost that key on a launch where
Schemaic had added nothing and withdrawn nothing
(`an_empty_container_we_did_not_empty_is_left_where_it_was`); and an **unparseable
document is declined** rather than overwritten — the cost of that is one session without
database tools, and the cost of the alternative is the user's file. An empty or missing file is
an empty document, because that is what a fresh install looks like and refusing it would deny
tools to exactly the people who have configured nothing. One limit stated rather than hidden:
removal matches by value, so a rule the *user* added by hand for the same tool is
indistinguishable from ours and goes with it — the alternative is a standing grant nobody
remembers making.
**A grant has to describe the level, not the session, which is why `install` calls
`_with_only_rules`.** Adding alone is not enough on a respawn that *lowers* the connection's
access: the previous session's wider rules are withdrawn by its own `Drop`, which runs on the
session task while the new `install` runs on a blocking thread, and if the new claim lands first
the old `Drop`'s `may_release` sees a claim that is not its own and withdraws nothing. Lowering a
connection from Full to schema-only therefore left `mcp(schemaic/run_query)` standing or not
depending on which thread won, while `antigravity_allow_rules`' own doc says a schema-only
connection never grants it. `_with_only_rules` clears the rules Schemaic could have granted and
is not granting now, then adds this level's — a statement about the *file* rather than about a
sequence of edits, so it holds whichever ordering wins. `ours` is the full set of our rules, so a
hand-written rule for one of our tools goes with it: the same documented limit as `_without_rules`
and the same trade. It stays idempotent — asking for the set already there answers `Unchanged`, so
a relaunch still rewrites nothing
(`granting_a_narrower_set_withdraws_the_wider_one_it_replaces`,
`granting_a_narrower_set_leaves_another_vendors_rules_alone`).
**"Nothing changed" is this layer's answer to give, because nothing outside it can compute
one.** All three return `Option<SettingsEdit>` — `None` is still the decline, and otherwise
`Unchanged` or `Write(text)` — and `app/antigravity.rs` writes only on the second. The caller
used to decide by comparing the returned text against the bytes it had read, a comparison that
never succeeds against a real file: `to_string_pretty` emits no trailing newline, and with no
`preserve_order` feature in this workspace `serde_json::Map` is a `BTreeMap`, so the user's keys
come back sorted. So every Antigravity user had another vendor's `settings.json` truncated,
re-sorted and rewritten at **every** launch — including everyone who never opened the AI panel,
since the startup sweep withdraws rules that are usually not there. A rewrite is not free even
when the content is equivalent: it re-sorts keys, strips the trailing newline, collapses CRLF,
normalises numbers and collapses duplicate keys — a fair price for an edit the user asked for,
and no price at all is the right one for an edit that removes nothing
(`withdrawing_from_a_file_that_holds_none_of_our_rules_changes_nothing`,
`granting_a_rule_that_is_already_there_changes_nothing`). `render_settings` is the single exit,
and it adds the trailing newline every editor and CLI that writes this file leaves on it.
**`opencode_config_json` is the same division for the harness whose whole configuration is one
file**: the JSON is built and tested here, and `app/opencode.rs` does the IO. It holds both
halves at once — the `mcp` table naming our server and its `--endpoint-file` command line, and
the `agent` entry whose `tools` map is the seal — which is Claude's `--mcp-config` plus
`--tools ""` folded into a single document. The connection's access level reaches it the way it
reaches every other harness: the tools it may call are named, by their **bare** names via
`bare_tool_name` — which this sentence claimed while the function inlined that helper's body
twelve lines from the two call sites that really did call it, so the single source of truth the
claim rests on did not exist; it calls it now — in the agent's `description` rather than in a
permission rule, because OpenCode
allows MCP tools by default and there is no per-tool approval to set; the server itself still
refuses anything the level does not offer, and naming them only saves the model a turn spent
discovering that (`a_read_only_connection_never_advertises_run_query`).
Two public constants sit beside it and are deliberately **not** one. `OPENCODE_AGENT` is the
agent this file defines and `turn_args` selects, so a rename cannot make the definition and the
selection disagree — and a disagreement there is silent, since the run falls back to `build`.
`MCP_SERVER` is the name the server is registered under, and it exists because **one dialect has
to decode a name Schemaic itself wrote**: OpenCode flattens the server into the tool name, so
`stream.rs` reads `schemaic_run_query` back as `mcp__schemaic__run_query` off this prefix. They
are the same string today and answer different questions — the agent is the tool *set*, this is
the tool *source* — and collapsing them is how one rename quietly becomes two. It has a third
consumer now: `antigravity_allow_rules` interpolates it, and `app/antigravity::SERVER` **is** it,
so the name `agy mcp add` registers and the name every allow-rule points at are one definition.
`TurnSpec` and `turn_args` build one turn's command line — one struct rather than eight
positional arguments, because the harnesses draw on overlapping subsets of it and a positional
list is how `--model`'s value ends up in `--effort`'s slot. **There are two spawn shapes and one
struct feeds both**: `turn_args` for the harnesses spawned per turn, `session_args` for the ones
spawned once per *conversation* with their turns arriving on stdin. Each returns an empty vector
for a harness belonging to the other, so a caller that asks the wrong one gets nothing rather
than a plausible command line for the wrong shape, and
`the_two_spawn_shapes_do_not_overlap` walks `Harness::ALL` holding both builders to
`is_persistent`. Claude's `session_args` arm delegates to `build_session_args`, which is what
that function always was. **Where the prompt sits differs and is not a detail to generalise from
one of them**: OpenCode takes it as the last *positional*, last on purpose since a flag after it
reads as part of it; neither persistent harness carries a prompt in argv at all; and **Codex
takes it on stdin**, which is `Harness::prompt_on_stdin`. That capability was true of Codex alone,
on the strength of `codex exec --help` (codex-cli 0.153.4): instructions are read from stdin when
no positional is given, *and* a piped stdin alongside a positional is appended as a `<stdin>`
block rather than obeyed — so the prompt has to **leave** argv rather than merely be duplicated
onto it. The reason is that argv is world-readable and the prompt is the payload: on the packaged
Linux targets `/proc/<pid>/cmdline` is readable by any local account, so a `ps auxww` during a
turn read up to `ATTACH_ROW_CAP` attached rows, an AI Seed's twenty sampled rows and the table's
full `CREATE TABLE` — a judgement this codebase had already made for the MCP endpoint
(`codex_mcp_overrides` keeps it out of `-c` for exactly that) while the rows the `AiData` ladder
exists to protect went to the same place. **Copilot is the second harness it is true of, measured
on 1.0.88 (2026-09-26)**: with stdin piped and no `-p`, the piped text *is* the prompt — the
`user.message` event echoed it back as its `content`, on the `--output-format json` turn and the
`-s` one-shot alike. It matters more there than for Codex, because on Windows an npm install puts
`copilot.cmd` on `PATH` and a multi-line argument through a batch shim is refused outright (see
`agent_cli.rs`), so `-p` would not merely publish the prompt but fail it
(`the_prompt_never_reaches_the_command_line`). **Cursor is the third, measured 2026-09-26**: `-p`
with no value and the prompt piped in answered the piped text, on `stream-json` and `text`
output alike — and `-p` must then be followed by a flag, never by text, which its own
`the_prompt_never_reaches_the_command_line` pins. The other three keep argv and each reason is
written at the capability: OpenCode's `run` documents a `message..` positional and no stdin, Antigravity
reads stdin only under its *session* shape, and Claude's stdin form passes context *alongside* a
positional rather than replacing one. **OpenCode's leg carries a date now — re-measured
2026-09-15**, `opencode run --help` still listing `message..` as a positional with none of its
twenty-odd options taking the prompt on stdin. Recorded with the date because this is the second
review to raise the question and both times the answer cost one `--help`; Claude's and
Antigravity's legs stay unverified *here*, neither binary being on the machine that measured the
others, and that is said rather than assumed. **What the three still on argv cost is stated at
the capability too**, since that is why it keeps being raised: `/proc/<pid>/cmdline` is
world-readable on Linux, so a one-shot turn's whole prompt — attached grid rows and Fill/Seed's
sample included — is visible to every local account for the life of the process. **Claude's
persistent session is on argv too, for its system context rather than its turns**:
`build_session_args` passes the system context, schema outline included, as the value of
`--append-system-prompt`, so the outline is readable the same way for the whole life of the
session process — the "no prompt in argv" said of the persistent harnesses above is about the
turns alone. Everything is
ready for a flip: `stdin_prompt` is threaded through `InlinePlan`, `run_inline` pipes it whenever
the capability is true, and `turn_stdin_prompt` covers the persistent shape, so moving one leg is
this `match` arm and a measurement against the installed binary. `turn_stdin_prompt` and `inline_stdin_prompt` build what
goes there from the same `prefixed_prompt` call the argv arm would have pushed — one
construction, two destinations — and both Codex arms, like both Copilot and both Cursor arms,
omit the positional under the same predicate, which is what `the_prompt_travels_exactly_once` and
`an_inline_prompt_travels_exactly_once` hold together over `Harness::ALL` (each asserts the two
are exclusive, so a harness that claims stdin and still pushes sends the payload twice, and one
that lost its push sends nothing).
**Antigravity is a persistent harness now, and that was measured before it was driven.**
`--input-format stream-json` reads one NDJSON message per line and runs a turn for each, which is
what the flag's own help promises and not the same thing as having watched it work: a two-turn
probe against the installed binary kept one process alive across both turns, held the same
`conversation_id`, counted `num_turns` up, answered the second question from the first one's
context, and took 4.96 s for the pair. That was then repeated end to end through this code's own
path — `session_args`, the app's session loop, the parser — before any of it was written down
(`antigravity_holds_the_conversation_like_claude_does`).
**`supports_resume` is no longer the complement of `is_persistent`, and the reason is that this
harness has no interrupt.** While Claude was the only persistent one the two were a single
question asked twice — a process that holds the conversation has no id to resume from — so
resume was literally `!is_persistent()`. Antigravity holds the conversation *and* has no stdin
message that ends a turn in flight, so Stop has to end the process and the next turn picks the
conversation back up with `--conversation <id>`: it needs both answers true. Each is an explicit
per-harness match now (Claude alone answers `false` to resume), and the old
`resume_is_the_complement_of_a_persistent_pipe` is replaced by the weaker rule that survives it,
`every_harness_can_continue_a_conversation_somehow` — every harness has one of the two
mechanisms — plus `persistence_and_resume_are_no_longer_complements`, which pins the combination
that used to be impossible.
**Three capabilities exist only for the persistent shape, and each was measured rather than
guessed.** `session_turn_line` is the stdin encoding: Antigravity's envelope is
`{"event":"user","message":{"content":…}}`, mirroring what it *writes*, and Claude's
`{"type":"user"}` is refused outright — *"stream input message is missing the `event` field"* —
so the two dialects cannot share an encoder
(`each_persistent_harness_encodes_a_turn_its_own_way`). `session_interrupt` answers `Some` for
Claude and **`None` for everyone else**, which is the honest answer and not a gap: an
unrecognised event on `agy`'s stdin is ignored *in silence*, so a guessed `interrupt` would not
fail loudly — the turn would run on with the panel waiting for a stop that never came
(`only_claude_can_interrupt_a_turn_in_flight`). `None` sends the caller to killing the child,
which is what makes `supports_resume` true above. `session_system_in_first_turn` is true for
Antigravity alone, which has no `--append-system-prompt`: the outline travels in the first turn's
text, once, since re-sending the whole schema outline on every question is most of what holding
one process was for (`only_the_harness_without_a_system_flag_folds_it_into_the_first_turn`).
**The model is trimmed once at the top of `turn_args`, because the field it comes from is free
text the user can clear.** Typing a space and closing the settings modal leaves `" "`, which an
`!is_empty()` waves through as `--model " "` — an unknown model, killing the spawn and reported to
the user as "Couldn't launch the CLI", which is the one explanation that is not the problem.
`build_session_args` already trimmed on Claude's path and these three did not, and that divergence
between the two argv builders was the whole bug.
`a_whitespace_only_model_is_no_model_on_every_harness` loops `Harness::ALL` and also asserts no
*other* blank argument reached argv, while `a_model_with_surrounding_space_is_sent_trimmed` holds
the direction that would break by over-trimming — and that one picks its builder off
`is_persistent`, because `session_args` trims at its own top for the same reason and the property
belongs to the field rather than to either builder.
**A loop over `Harness::ALL` that calls `turn_args` is vacuous for half the enum, and two tests
here were.** `turn_args` answers `Vec::new()` for Claude and Antigravity, so "no `--model`
appears" held by there being no argv at all, under docstrings that said *every* harness —
deleting `!model.is_empty()` from `session_args`'s Antigravity arm kept the whole suite green,
and `agy … --model ""` then dies as "Couldn't launch the CLI". The test helper `spawn_argv(h,
spec)` is the fix: it picks each harness's **own** shape (`session_args` for the persistent two,
`turn_args` for the others) and asserts the argv is non-empty *before* anything else is checked,
so a loop over it cannot go vacuous again. Both tests above go through it, and
`an_empty_model_lets_the_harness_keep_its_own_default` now also asserts the positive direction —
a real id does reach every harness's argv — so the guard cannot be satisfied by dropping the
flag altogether.
**The system context rides the first turn of a thread and none of the rest**, which is
`turn_system`. Codex has no `--append-system-prompt` and none is passed
to OpenCode either, so for both `prefixed_prompt` folds
the schema outline into the prompt itself (with a blank line between, or the outline's last table
name reads as the first word of the question) — and a resumed turn makes the CLI replay the whole
prior thread, every turn of which already carries its own copy. Sent every time, turn N put the
outline into the model's context N times: a ten-turn conversation against a large catalogue paid
for it ten times, and the panel's own input-token count would show it climbing against a
catalogue that never changed. The rule lives here beside the argv rather than at the call site
that fills in `TurnSpec`, so the caller cannot forget it, and the tests run the composition
rather than the predicate
(`the_schema_outline_rides_the_first_turn_of_a_thread_and_not_the_rest`, which loops those two —
Antigravity was a third until it became persistent and now reaches the same rule by the other
road, `session_system_in_first_turn` above; an empty
`resume` is not a resume and still carries the outline —
`an_empty_resume_id_still_carries_the_outline`). **Every flag in the Codex argv was verified
against the installed binary's `codex exec --help`**: `exec`, `--json`, `-s/--sandbox` with
`read-only|workspace-write|danger-full-access`, `--skip-git-repo-check` (Codex wants a git repo
and the session cwd is a private app directory that is not one), `-m/--model`, `-c/--config`,
`--ignore-user-config`, and the `resume` sub-subcommand, which must follow `exec` immediately
(`codex_resume_follows_exec_immediately`). `-p` on `codex` is **`--profile`, not a prompt
flag** — the one name that reads like Claude's and means something else.
**Codex's constraint travels as a config key rather than as the flag**, and the flag is defence
in depth on the one path that takes it: `--sandbox` exists on `codex exec` but **not** on
`codex exec resume` (measured against the installed binary), so a flag-only constraint would
either kill every resumed turn on an unknown option or leave every turn after the first
unconstrained. `-c sandbox_mode="read-only"` is accepted on both paths, so it is the primary
mechanism and `--sandbox read-only` is added only when not resuming. **It is emitted last among
the `-c`s**, because `-c` precedence is positional in the direction that surprises: Codex
splices root overrides to the front expressly *"so they have lower precedence than
command-specific flags parsed after a subcommand"*, which is to say a **later** `-c` wins. Put
before `mcp_overrides`, a caller passing its own `sandbox_mode` would silently outrank the
constraint.
**The rest of that page has been read now, and it closes favourably.** `--sandbox`'s absence
from `codex exec resume` was for a long time the only thing anybody had read off the binary; the
other four flags a resumed turn carries — `--json`, `--skip-git-repo-check`, `--model` and, when
the probe saw it, `--ignore-user-config` — were taken from the `exec` page, which is the very
page that also carries `--sandbox`, so the one flag that had been looked up was the one proving
the two pages differ. `codex exec resume --help` on **codex-cli 0.153.4** lists `-c/--config`,
`--last`, `--all`, `--enable`, `--disable`, `-i/--image`, `--strict-config`, `-m/--model`,
`--dangerously-bypass-approvals-and-sandbox`, `--dangerously-bypass-hook-trust`,
`--thread-source`, `--skip-git-repo-check`, `--ephemeral`, `--ignore-user-config`,
`--ignore-rules`, `--output-schema`, `--json`, `-o/--output-last-message` and `-h/--help` — and
no `--sandbox`, confirming the one measurement that had been taken. So every flag `turn_args`
puts on a resumed turn is real — those four, and the `-c` that carries both the caller's
overrides and the `sandbox_mode` constraint — and the failure this was a risk of, one unknown
option killing every Codex turn after the first and being reported as *"The Codex turn ended
unexpectedly"*, was never live.
`a_resumed_codex_turn_passes_only_flags_that_subcommand_takes` holds the argv inside that list
and separately asserts the four are present, so it cannot pass by emitting nothing; it was
watched failing against a `turn_args` that passes `--sandbox` on resume. Its `RESUME_FLAGS`
const is a transcript of that help page and **has to be re-read from `--help` when the CLI is
upgraded** — that is the point of holding it as a const, not a maintenance smell to be replaced
with something cleverer.
**The Antigravity argv lives in `session_args` now and leads with `--input-format stream-json
--output-format stream-json`** — the second required by the first, on that flag's own help. Its
two flags of evidence carry over unchanged from the per-turn argv it replaced: `--sandbox`
(*"Run in a sandbox with terminal restrictions enabled"*) and `--disable-slash-commands`
(*"Disable slash command and skill expansion in print mode"*) are both verbatim in the captured
`agy --help` **and** were passed in every live run, including the two that produced the stream
fixtures — each exited 0 and returned normal output, so they are accepted rather than merely
advertised. That matters twice for `--sandbox`: it is precisely because it was passed in those
runs that the 56-tool `init` listing is evidence of a ceiling rather than of a flag nobody set.
The second is passed because a prompt is *user text* that must not be able to invoke a slash
command or a skill.
**`-p` is gone rather than emptied, and that is measured.** It takes the prompt as the flag's own
*value*, so leaving it in with nothing to give makes the CLI read the next flag as the prompt —
the binary says so itself: *"-p took `--input-format` as its prompt"*. In bidirectional mode the
prompt arrives on stdin, so the flag goes entirely
(`a_persistent_antigravity_is_spawned_with_no_prompt_flag`). `--model` and `--effort`
(`low|medium|high`, which is this harness's `effort_levels`, and a level only Claude takes is
dropped by `effort_arg` before it reaches here) ride along as they did.
**`--conversation <ID>` is measured against the installed binary now, and so is the whole
Stop-then-resume path** (*"Resume a previous conversation by ID"*, with `-c/--continue` beside it
for the most recent). It is emitted only when `TurnSpec::resume` is non-empty, which on this path
means only after a Stop had to kill the process — so the probe did exactly that: turn one on one
process stored a number and reported `conversation_id` `dad00e0a-…`, that process was killed,
which is what Stop does, and a fresh one spawned with `--conversation <that id>` last in the argv,
after `--model` and `--effort`, where `session_args` emits it. It answered the follow-up with the
remembered number, reporting `num_turns: 2` and the *same* `conversation_id`. So the id we feed
it — the top-level `conversation_id` off `init` — is the id it wants, and the flag composes with
the bidirectional flags rather than being refused beside them; the recovery is verified end to
end rather than inferred from the two halves. And the help itself is read
off **stderr**, where `agy` writes it while exiting 0 — a stdout-only probe finds it empty and
refuses a working install, which is why `agent_cli::probe` folds both streams.
**The OpenCode argv is `run --pure --agent schemaic --format json`, and two of those flags do
more than they look.** `--agent` is what selects the sealed agent, so it is not decoration around
the config file but the half that activates it; drop it and the turn runs as `build` with every
built-in (`a_turn_runs_the_sealed_agent_and_asks_for_json`, and
`the_agent_the_config_defines_is_the_one_the_argv_selects` under `app/opencode.rs`, which walks
the argv's `--agent` value back into the JSON and checks *that* agent is the one with the tools
switched off — a `const` on both sides can never break the lookup, but the JSON growing a
different shape around it can). `--pure` is the plugin half of the seal *and* the reason a first
turn finishes: see `app/opencode.rs` for the three-minute silent bootstrap it skips. `--model`
and `--variant` are omitted when empty, `--session <id>` is the resume, and the prompt is the
last positional as Codex's is.
**No `--auto`, and the reason is the opposite of Antigravity's.** Measured, a headless turn under
this agent called its tool and returned without ever asking permission, because OpenCode's
default permission set allows rather than denies; Antigravity's auto-*denies*, which is the whole
reason `AgyRegistration`'s allow-rules exist. Its own help calls the flag dangerous, and passing
it would buy nothing while pre-approving whatever a future build adds to the tool set
(`a_turn_never_passes_the_auto_approve_flag`).
**The Copilot argv is `--output-format json --stream on`, the allowlist, `--disable-builtin-mcps
--no-custom-instructions --no-auto-update --no-ask-user`, and every value a flag takes is joined
with `=`.** That is not style: `--available-tools` and `--allow-tool` are variadic
(`[<tools>...]`) and `--resume` takes an optional value, so a space-separated value is one parse
decision away from swallowing the argument after it
(`every_variadic_value_is_bound_with_an_equals_sign`). When the session has a config,
`--additional-mcp-config=@<file>` adds our server and `--allow-tool=schemaic` approves it, and
those are two questions rather than one: the allowlist decides what the model can *see*, this
decides what runs without a prompt, and headless there is no prompt. Measured, without it the call
is not made; with it, `--allow-all-tools` — which the help calls "required for non-interactive
mode" — is not needed, and because it names the server it can only approve what the allowlist
already let through. No config means no approval either, so no grant sits waiting for whatever
takes the name (`our_server_is_configured_by_file_and_approved_by_name`), and
`nothing_that_widens_the_seal_is_ever_passed` holds both argvs free of `--allow-all-tools`,
`--yolo`, `--autopilot` and their kin. `--no-auto-update` runs the build that was probed rather
than one downloaded mid-session whose flags nobody has read; `--no-custom-instructions` is defence
in depth behind an empty cwd, as Claude's `--setting-sources` is. **The resume is
`--resume=<id>`, not `--session-id`**, though both continue a session (measured):
`--session-id` also *creates* one when the id is unknown, so a conversation whose state had gone
would silently restart with no memory — on a turn `turn_system` has already stripped of the schema
outline. A resume that fails is a turn that says so
(`a_resumed_turn_resumes_and_drops_the_outline`). No `--reasoning-effort`, for `effort_levels`'
reason, and no prompt, which is on stdin. The CLI also advertises a persistent mode, `--acp`, and
it is not what is driven, for the reason Antigravity's bidirectional mode waited: it is not what
was measured. **Measured end to end, by the author of the change, on 1.0.88 under Windows 11
(2026-09-26)**: against the real `--mcp-serve` server over a SQLite endpoint the server
connected, `run_query` ran, a resumed second turn recalled the first, and a schema-only
allowlist left three tools and a model that said it could not run a query.
**The Cursor argv is `-p --output-format stream-json --stream-partial-output --trust --mode ask`,
then `--model`, then `--resume <id>` last**, and most of what matters about it is what is not
there. `-p` takes no value because the prompt is on stdin. `--trust` is there because a headless
run in a directory the CLI has not seen exits at a trust prompt, and trusting this one grants
nothing: it is Schemaic's own, holding only the two files that restrict and configure the session.
`--mode ask` is the prompt described above, not a lever. `--resume` goes last because its value
is optional, so nothing may follow it that could be read as the id, and in the space-separated
form because that is the one measured (`a_resumed_turn_names_its_chat_last_and_drops_the_outline`).
What restricts and configures the turn is not on the command line at all but in its working
directory, which is why `ai.rs` starts this harness's child somewhere other than `session_cwd`.
**Measured end to end on 2026.09.23-86fc751 under Windows 11 (2026-09-26)**, with this argv against
the real `--mcp-serve` server over a SQLite endpoint: `run_query` ran, a resumed turn recalled
the first, and a request to write was declined.
**`inline_argv` is the closed counterpart of `turn_args`, and "closed" is the whole
specification.** Ctrl+K, AI Fill and AI Seed each want one string back that a parser then reads,
so `InlineSpec` carries intent, system, model, effort, seal, `isolate_config` and Codex's
`last_message` and nothing else — there is no field for a server, an approval map or a resume id,
because no arm may have one. On every harness the argv is free of `--mcp-config`, of any `-c`
that names a server, and of anything that resumes a thread this one-shot never saw
(`no_inline_generation_is_given_a_server_or_a_session` runs the forbidden list over
`Harness::ALL`, narrowing Codex's permitted `-c`s to `sandbox_mode=` and the empty
`mcp_servers={}` rather than exempting the flag; it drives **both polarities** of
`isolate_config`, and the reason is the finding below). Claude's arm delegates to
`crate::inline_args`, which gained an `effort` parameter and is
otherwise what it was: it was the whole of this path for as long as the other three spawned Claude
regardless of the picker, and is now one of six.
Measured against the installed binaries, Codex is `exec --ephemeral --skip-git-repo-check
--sandbox read-only --color never`, then `--ignore-user-config` when the probe saw it, then
`--model`, then `-c sandbox_mode="read-only"`, then `-c mcp_servers={}`, then `-o <file>` — and
**no prompt at all**, `inline_stdin_prompt` carrying it on stdin (`Harness::prompt_on_stdin`);
Antigravity is `-p <prompt> --output-format text --sandbox --disable-slash-commands`
plus `--model` and `--effort`; OpenCode is `run --pure --agent schemaic --format default` plus
`--model` and `--variant`, prompt last; Copilot is `-s --output-format text --no-color`, then
`--available-tools=schemaic-no_tools` — an allowlist that allows nothing, never an absent or
empty one, for the fail-open above — and the same `--disable-builtin-mcps
--no-custom-instructions --no-auto-update --no-ask-user` as its session, plus `--model`, with
the prompt on stdin; Cursor is its session's argv with `text` for the stream and no resume,
`-p --output-format text --trust --mode ask` plus `--model`, the prompt on stdin, run in a
`cursor-inline` workspace whose permissions file allows nothing and configures no server
(`a_one_shot_asks_for_text_and_carries_no_session`).
**`--output-format text` is named on Antigravity and Copilot although it is
the default**, because the session path asks that same binary for a JSON stream and a
default that moved would put a JSONL envelope where a parser expects SQL.
**`-c mcp_servers={}` is on this path now, and its absence was the release review's `S2-L5-01`.**
A Codex one-shot emitted `-c sandbox_mode="read-only"` and no `mcp_servers` override at all, so
on a build advertising `--sandbox` but not `--ignore-user-config` — still graded `Restricted`,
still runnable, and exactly the build the flag is conditional *for* —
Ctrl+K loaded and launched **every server in the user's own `~/.codex/config.toml`** and offered
their tools to a generation with no surface on which a tool call could appear. The session path
prevents that unconditionally. It is emitted here whether or not `--ignore-user-config` is
available, because the flag is the one that may be missing and the config key is the mechanism
that is not, and it is the same `codex_isolation_only()` the session's fallback assigns, so the
two cannot drift into different ideas of "no servers"
(`a_codex_one_shot_displaces_the_users_own_servers_with_or_without_the_flag`, which also holds
the flag itself to tracking the probe, so this is not covering for a flag that quietly stopped
being passed).
**The test that should have caught it rejected the fix instead, which is `S2-L6-01`.**
`no_inline_generation_is_given_a_server_or_a_session` asserted that every Codex `-c` starts with
`sandbox_mode=`, so the correct argv made it panic; and `inline_tests::spec()` hard-coded
`isolate_config: true`, so no inline test ever built the un-isolated argv — which is precisely
the state the bug bit in. The forbidden-flag rule was reaching for "no `-c` hands the model a
server", and an empty table is the honest version of *no database tools*: ours absent, and
nobody else's in its place. Both polarities of `isolate_config` are looped now, and the empty
table is allowed by name rather than by prefix.
**Effort reaches a one-shot now, and `inline_argv` clamps it itself.** It never did on Claude's
path, so a setting the modal presents as the assistant's applied to the chat panel alone; each
harness takes it in its own flag — `--effort` on Claude and Antigravity, `--variant` on OpenCode,
nothing at all on Codex, whose `effort_levels()` is empty and which therefore gets no flag rather
than a dropped one, and nothing on Copilot or Cursor either, whose lists are empty for the
reasons above (`effort_reaches_each_harness_in_its_own_flag`). **The clamp lives in the
argv builder rather than at the caller, and all three builders do it now.** It was this one
alone — three call sites reach `inline_argv` and each would have had to remember — while
`turn_args` and `session_args` were handed an already-answered `effort_arg` by their callers in
`ai.rs`. That divergence is gone: each of the three asks `h.effort_arg(spec.effort.trim())` at
its own top, idempotently, so a caller that still clamps costs nothing and one that stops costs
nothing either. The gap was not theoretical — `turn_args(OpenCode, effort: "xhigh")` really did
return `--variant xhigh`; both callers happened to clamp, so there was no wrong output and
nothing that could have caught it if one stopped. **And widening the test to every harness ×
every level found a second gap the review had not predicted**: it failed on Claude, because
`session_args` did not clamp at all, so a Claude session would have been spawned with
`--effort minimal` — a level that flag does not advertise, Claude's list being
`low|medium|high|xhigh` — the moment the setting carried `minimal` over from OpenCode. So
`InlineSpec::effort` is the level as the user set it, in whatever vocabulary the harness they
set it under uses. The setting outlives a harness switch,
so Claude's `xhigh` under Antigravity is the ordinary case and not the exotic one, and it is
dropped rather than sent (`an_effort_the_harness_never_advertised_is_not_sent`). `inline_args`
says in its own doc that the value arrives clamped, because `inline_argv` is its only caller and
a second one would have to do the same.
**Codex is the only harness whose reply is read from a file, and that is a contract rather than an
observation.** `inline_output` is where that lives — `LastMessageFile` for Codex, `Stdout` for the
other five — and the reason is that `codex exec` without `--json` prints *for a person*:
measured, this build writes the final message alone, but that is a statement about a human-facing
surface and not a promise, while `-o/--output-last-message` is documented as the file the last
message is written to. `--color never` rides along for the same reason: escapes in a rendering
nobody parses cost nothing, and escapes in a reply that lands in the editor do. OpenCode needs no
such split — measured, its decoration and model banner go to **stderr** and stdout carries the
answer alone — and neither does Copilot, whose `-s` is "output only the agent response (no
stats)" and measured as exactly that, one line of SQL, nor Cursor, whose `--output-format text`
was measured as the answer alone, a fenced SQL block that `extract_sql` unwraps
(`codex_writes_its_last_message_where_it_was_told_to`,
`every_inline_generation_asks_for_plain_text`).
**OpenCode's one-shot gets its own config, and the difference from the session's is the whole
`mcp` block.** `opencode_inline_config_json` defines the same sealed `schemaic` agent and
registers no server at all: a chat turn is meant to reach the database, while a one-shot answers
from the prompt and has nowhere to render a tool call, so a server here would be a tool handed to
the one path that could not show it had been used. The agent is still defined, because without it
the `--agent` in the argv selects nothing and OpenCode falls back to `build`, which has every
built-in (`the_inline_opencode_config_defines_the_agent_and_no_server`). The twelve-name tool map
is `sealed_tools`, extracted so this config and `opencode_config_json` cannot come to disagree
about which built-ins are off — the same reason `seal_args` is shared by Claude's two argv
builders — and the denylist-by-omission caveat above applies to both of them equally.
**One hole the argv cannot close, and it is Antigravity's — measured now, not inferred.**
Everything above is a guarantee about a command line, and Antigravity is the harness with no
per-invocation configuration at all: its server is registered *globally* by `AgyRegistration` for
the life of a chat session. Measured against the installed `agy`, with a server registered through
`agy mcp add`, `agy -p` — print mode, exactly what `inline_argv` builds — lists that server's tools
among its own, `list_schema`, `describe_table` and `propose_table_change` included; with no
registration it lists Antigravity's built-ins alone. So "no inline generation is given a server"
genuinely does not hold for Antigravity while a chat session holds the registration. It does hold
for four of the other five, which are configured per invocation (Copilot's one-shot runs in a home of
its own, `copilot-inline`, so a live session's `schemaic-mcp.json` is not even lying beside it),
and this is written down because that sentence is exactly the one that would otherwise be read
as covering all six. **Cursor's holds for our server and not for the user's**: its one-shot runs
in a `cursor-inline` workspace with no `mcp.json`, so a live session's server is not in reach,
but the servers in the user's own `~/.cursor/mcp.json` are started on every run — a one-shot
included — their tools refused unless the user's own settings allow them (see
`isolates_mcp_servers`).
**It did not hold for Codex either, and this paragraph said it did.** Stating the seal as settled
fact "for the other three" was false whenever `isolate_config` was false: with no
`--ignore-user-config` and no `mcp_servers` override, the user's own servers loaded into a
one-shot. That is the `-c mcp_servers={}` fix above, and the sentence is only true now because
of it — which is the point of recording it here rather than only there. Antigravity remains the
one genuine exception, for the reason no flag can take away.
**And there is no per-invocation fix in that CLI to reach for, so the limit stands as
described.** It has no config flag for this, and no definable agent the way OpenCode has —
`agy agent` only lists agents, and lists none. The one alternative was to refuse a one-shot
while an Antigravity chat session is live, and that is decided against: the inline prompt asks
for SQL and nothing else, and Ctrl+K, AI Fill and AI Seed are keystrokes — refusing one because
a panel happens to be open would cost more, on every press, than the narrow overlap it removes.
So this is a settled limit rather than unfinished wiring, and nothing in the code gates on it:
this paragraph is the whole of the record.
**What this paragraph used to get wrong is who else is inside that window.** It bounded the
exposure as "the window in which a chat session holds the registration and no wider", which is
true of *time* and was silently read as a statement about *processes* — it asked which of
Schemaic's own spawns could see the server and never asked which other ones could. The
registration is in the user's own MCP config and `permissions.allow` is an allow-list rather than
a prompt-list, so for as long as that window is open **any** `agy` run by this user, in any
directory, started by anything, finds the `schemaic` server registered and its database tools
pre-approved with no prompt: `cd ~/work/some-repo && agy -p "explain this build failure"` can
reach the database, steered by a `README` or `AGENTS.md` the user did not write. No lever was
found that scopes either half to one process, and a per-session server *name* was considered and
rejected — an unrelated run inherits whatever name is registered, so it would close the
name-collision limit under `app/antigravity.rs` and not this one. What is left is to keep the
window as narrow as it can be (installed at session start, withdrawn at session end, covering
only the tools that connection's access level offers) and to say so, which
`Constraint::notice` now does in the panel.
**`cli_failure_message` takes the harness now, and that is not decoration.** Its last-resort arm
said *"the claude CLI exited with status N"* whatever had run, so a user who had picked Codex was
sent to check an installation that was not the one that failed — the same wrong-cause report the
Claude-only inline rule existed to prevent, arriving by a different door the moment all four
harnesses could run a one-shot. The stderr → stdout → status order is unchanged, and it is that
order because the CLI writes some fatal errors (an expired OAuth session among them) to stdout
with an empty stderr, so surfacing stderr alone yields a blank. On Claude Code 2.1.289 a
`stream-json` session's expired login does not reach this order at all: it arrives as JSON in a
turn that ends cleanly, and is read out of that turn's `result` (`claude_error_text`, under
`stream.rs`). What it answers is a `CliFailure` rather than a `String` (under `lib.rs`), so the
two fallbacks can be worded in the interface's language while what the CLI said stays its own.
With the Gemini adapter deleted, `claude`, `codex`, `agy` and `opencode` were the four, and every
argv here is for a binary somebody has run — and with the Antigravity resume measured above,
nothing on any of those four is now carried on its help page alone. `copilot` is the fifth, and
every flag in both of its argvs was in a measured run (1.0.88): the session's four `--no-…`
flags in the end-to-end turns against the real server, and the one-shot's `--no-color` beside
them in a `-s` run that answered with SQL alone. `cursor-agent` is the sixth, and both of its
argvs were run as written (2026.09.23-86fc751): the session's in the end-to-end turns against the
real server, the one-shot's in a `text` run that answered with a fenced SQL block. The one exception is the
`codex exec resume` flag list described above, which is carried on *neither*: no page was read
and no run was recorded.
