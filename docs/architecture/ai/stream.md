# `ai/stream.rs`

`stream.rs` — one transcript vocabulary, six CLI dialects. Every harness decodes into the same
`StreamEvent`s the panel already renders, so the dialect stops at this module and nothing
downstream learns which CLI produced a turn. **They do not even agree on where the discriminator
lives**: Claude, Codex, OpenCode, Copilot and Cursor tag a line with `type` (Copilot nesting
everything else under `data`), while Antigravity tags it with `event`
and nests the payload under a key of the same name (`{"event":"init","init":{…}}`,
`{"event":"step_update","step_update":{…}}`). A line shaped like the other three decodes to
nothing at all, which is why that is pinned rather than assumed
(`an_antigravity_event_is_keyed_on_event_not_type`) — and it is what the binary emitted, not
what its changelog implied.
**Decoding needs state, which is why it is a `StreamParser` and not a function — and there are
two pieces of it.** The first is prose.
Claude and Antigravity stream *deltas* — each line carries only the text new since the last one,
so a stateless `line -> events` map is exact, and `parse_stream_value` is kept and delegated to
rather than generalised — Claude's arm, `push_claude`, adds only the failed turn's reason
beside it (see `claude_error_text` below). Antigravity's half of that is measured rather than assumed: a captured
answer split **mid-word** across two `text_delta`s (`"orders\nwidge"`, then `"ts\n"`), which is
what a partial chunk looks like and a cumulative restatement never does
(`real_antigravity_deltas_are_partial_chunks_and_append`). Codex *documents* re-sending a
message's whole text on every `item.updated` for the same item id — and that one, unlike the
Antigravity half beside it, is a contract rather than an observation: measured, this build never
restates at all, which is the finding in the confidence-boundary paragraph at the end of this
entry. Appended as they arrive, a three-chunk reply renders as
"Hi" + "Hi there" + "Hi there!"; the private `Coalescer` emits only the unseen suffix instead,
and falls back to the whole string when the text is *not* an extension of what came before, so a
rewritten message loses nothing rather than being diffed against a string it shares no prefix
with. It is keyed by ids unique only within one *turn* — the narrower unit, and see the
turn-boundary reset below for what made that distinction load-bearing — so a parser must not be
shared between two concurrent streams either: one per turn for Codex, OpenCode, Copilot and
Cursor, one per session for the two persistent harnesses.
**Copilot sends both shapes, which is why `Coalescer::extend` exists.** Its
`assistant.message_delta`s are true deltas, and then `assistant.message` restates the whole
message. `extend` records each delta against the message id as sent, so that when the
restatement is fed to `settle` under the same id it answers "nothing new" — or only the tail the
deltas never carried — and the key is cleared once the whole message has been seen. Printing the
deltas and ignoring the restatement would be silence with `--stream` off; printing both would be
the answer twice (`a_message_with_no_deltas_is_printed_from_its_restatement`,
`a_restatement_longer_than_its_deltas_adds_only_the_tail`). **`settle`, not `advance`**, because
`advance`'s non-extension arm is Codex's — a cumulative restatement that is not a prefix really is
new text, and is sent whole. For a dialect whose restatement is a *copy* of text already on
screen, one that is shorter, trimmed or normalised adds nothing, and sending it whole printed the
answer twice when a restatement dropped the deltas' trailing newline
(`a_restatement_shorter_than_its_deltas_adds_nothing`).
**Cursor sends both as well, in three shapes of one event, and the obvious rule for telling them
apart was watched failing.** Under `--stream-partial-output` every piece of prose is an
`assistant` event: deltas carrying `timestamp_ms` and no `model_call_id`; before a tool call, the
segment so far **restated whole**, carrying `timestamp_ms` *and* `model_call_id`; and at the end
the last segment restated whole with neither. "Timestamped means delta" prints the pre-tool
sentence twice. So `push_cursor` calls a line a delta only when it has `timestamp_ms` *without*
`model_call_id` and feeds it to `extend`; anything else is a restatement, fed to `settle` under
the same key and then cleared, so it prints only a tail the deltas never carried — nothing when
streaming, the whole segment when not (`a_segment_with_no_deltas_is_printed_from_its_restatement`,
`a_trimmed_final_restatement_is_not_printed_again`).
**The key is a segment, not a message**: Cursor gives prose no id, so a `tool_call` clears the one
key `cursor-segment` and starts the next, which is what makes each restatement comparable with
its own deltas rather than the turn's (`a_real_cursor_turn_prints_each_segment_once`).
**OpenCode sends neither, and that is structural rather than a sampling artefact.** Its printer
emits a `text` part only once `time.end` is set — once the part is *finished* — so a whole answer
arrives in one event, a measured 2964-character reply among them. `streams_deltas` is false for
it, and no amount of coalescing would help, because no partial text is ever written to decode.
The visible cost is that there is no "it has started" moment to report: the panel's spinner runs
until the text lands.
**`streams_deltas` is a record, not a lever, and its doc now says so.** Nothing in the app calls
it: it claimed the panel used it to decide whether a first token means "it has started", and the
panel decides that from `m.pending && m.segs.is_empty() && m.notes.is_empty()` — the better
rule, since it answers
correctly for the harness that streams nothing. It is kept with `#[allow(dead_code)]` rather
than deleted because it is a measured fact about six CLIs that this parser's whole design rests
on, and deleting a measurement to satisfy a warning is how a measurement gets taken twice.
`supports_resume` and `supports_model_choice` are uncalled the same way and were left alone.
**There are two pieces of that state now, and the second is `seen_tools`.** Two dialects restate
a tool call while it is still running, neither marking the repeat —
Codex on every `item.updated` for the item id, Antigravity on every `state: "ACTIVE"` for the
`step_index` — while `TurnState::apply` pushes a chip for every `ToolUse` it is handed and, for
a result it cannot pair by id, attaches it to the *last* pending one. So a restated call left
the earlier chip spinning for the rest of the transcript. **Antigravity's half of that is measured and Codex's is
a documented contract**, on exactly the footing of the cumulative prose restatement it mirrors
(see the confidence-boundary paragraph at the end of this entry): the three codex-cli 0.153.4
turns that settled the prose question contained no tool call at all — no MCP server was
configured, and shell execution was refused before an item existed — so nothing has yet shown
Codex restating a call either. The guard is kept for the reason the `Coalescer` is kept: the
shape is one Codex documents, and being wrong in the other direction costs a chip that spins for
the rest of the transcript. The set holds whatever id that dialect gives the call,
and only the first sighting announces a chip (`a_restated_tool_call_does_not_add_a_second_chip`,
`a_restated_agy_tool_step_does_not_add_a_second_chip`, and
`two_distinct_tool_calls_still_get_a_chip_each` for the direction that would break by
over-suppressing). **An item with no id of its own is announced anyway, on both dialects**: every
measured Antigravity step carried a `step_index`, and silently dropping a call is the worse of
the two failures. Codex's arm answered the opposite way and did not say so — `codex_item` reads
the id as `unwrap_or("")`, so every id-less item in a turn shared the single key `""` (and
`"\0done"` for the completion half), and measured, two `mcp_tool_call` items with no `id`
produced **one** chip: the second query ran and left no trace at all, not a chip, not its SQL,
not its result. The same collision reaches `command_execution` and `file_change` through
`side_effect`. `first_sight` now answers true for an empty id and for the `"\0done"` an empty id
produces, neither of which a real id can reach. Defensive rather than observed — no captured
Codex turn omits `id` — and what it fixes is the direction the default fails in;
`codex_two_items_with_no_id_are_two_calls` returns `["A"]` against the old guard where
`["A", "B"]` is what ran.
OpenCode is the third and does not restate: a measured call arrived **once, already
`completed`**, with its `input` and its `output` in the same event, so `opencode_tool` emits the
chip and fills it from one line. It is keyed on `callID` all the same — the part id changes
between restatements where the call id does not — and the `running` status is handled, so a build
that starts streaming its calls costs one chip rather than a duplicate per event.
Copilot opens a call on `tool.execution_start` and closes it on `tool.execution_complete`, both
keyed on `toolCallId` and each through `first_sight` (`<id>\0done` for the close), so a restated
start or completion is one chip resolved once
(`a_restated_copilot_tool_call_announces_and_resolves_once` — defensive, since the captured turn
restates neither). A completion whose start never arrived opens its own chip — labelled `tool`,
since the completion carries no name — rather than emitting a loose result that `TurnState`
would attach to another call's still-open chip; that is `side_effect`'s rule for Codex, and this
arm lacked it until review (`a_completion_nothing_announced_does_not_resolve_another_call`,
watched failing with the running call's chip taking the second call's answer). `success: false` is what marks the chip an error; `copilot_result_text`
reads `error.message` first on the *assumption* that a failure names itself the way Codex's does
— only successes were captured — then `result.content`, then the MCP blocks in
`result.contents`, so a failure shaped some other way shows an empty result rather than a wrong
one. **The name is rebuilt from fields, not unflattened from `toolName`**: Copilot reports
`schemaic-list_schema` *and* `mcpServerName`/`mcpToolName` beside it, so `copilot_tool_name` is
Codex's rebuild rather than OpenCode's prefix-strip — splitting on the dash would misread any
server whose own name has one — and a built-in, which carries no server field, keeps the name it
really has (`a_builtin_tool_is_reported_under_its_own_name`).
Cursor sends a `tool_call` event `started` and one `completed`, keyed on `call_id` and each
through `first_sight` (`<id>\0done` for the close), and **its shapes are keys rather than
fields**: the payload sits under `tool_call.<kind>ToolCall` (`mcpToolCall`, `readToolCall`,
`shellToolCall`…), and the verdict under its `result` is a key too — `success`, or one of the
refusals `rejected`, `permissionDenied`, `writePermissionDenied`, `readPermissionDenied` and
`error`, all measured. `cursor_result` looks `success` up — the workspace's `serde_json` has no
`preserve_order`, so "the first key" is the alphabetically first, and a sibling such as
`isBackground` made a success a refusal named after it
(`a_success_beside_another_key_is_still_a_success`) — treats any other key as a refusal and shows
whichever reason field it carries, and reads an MCP result's text one level deeper than MCP's own
shape, `{"text":{"text":…}}` (`a_refused_call_is_a_refused_chip_with_its_reason`).
`cursor_tool_name` rebuilds an MCP call's name from `providerIdentifier` (or `serverIdentifier`)
and `toolName`, for Copilot's reason rather than splitting `schemaic-run_query`; a refused MCP
completion carries no `args` at all (measured), so one first seen that way is labelled `mcp`
rather than guessed. **Built-ins are shown under their kind, `grep`, `shell`, `edit`, because on
this harness the readers run unprompted** and a refused writer should be seen refused — the
one exception is `getMcpToolsToolCall`, the CLI looking up which MCP tools exist, which the model
does before nearly every call, so a chip for it would sit beside every real one saying nothing
(`a_builtin_reader_is_shown_and_the_catalogue_lookup_is_not`).
**All of that state is cleared at a turn boundary, and it only began mattering when a stream
could hold more than one Antigravity turn.** `StreamParser::push` looks at the events it has just
produced and, if any of them is a `TurnDone`, resets `seen_tools`, the `Coalescer`, the
`OpenCodeTurn` and `claude_spoke`. The ids `seen_tools` holds are whatever the dialect gives a call,
and they are unique only *within* a turn — Antigravity numbers its steps from zero on each one. That was
invisible while every multi-turn stream was Claude's, whose ids are unique for the life of the
process: on a persistent Antigravity the second turn's first tool call carries `step_index` 0
again, the set already holds it, and the chip announcing it is dropped — a tool call running with
nothing on screen to say so, which is the one failure this dialect is decoded carefully to avoid.
**`a_second_turn_reuses_step_ids_and_still_gets_its_chips` pins it, and it takes two turns to
do so** — every other tool test here drives one, which is exactly why the collision was
invisible. It feeds a tool call at `step_index` 0, a `result` closing the turn, then a
*different* call that is `step_index` 0 again through a single parser, and asserts the second
turn's chip is announced: with the reset disabled it fails with *"the second turn's tool ran
with no chip to show for it: []"*. It then restates that second call and asserts no second chip,
so the reset cannot be "fixed" by simply never de-duplicating.
**Both halves of a Codex `mcp_tool_call` are guarded now, and the result half was not.**
`seen_tools` kept a restated call from opening a second chip, but the completion path emitted its
`ToolResult` unconditionally — so a call whose *only* event is `item.completed` produced a result
with no chip to land in, and `TurnState::apply` attaches a loose result to the most recent call
still awaiting one. A database answer was therefore stamped onto whatever chip happened to be
open, a `command_execution` among them, or dropped outright when none was, leaving no record of a
query that ran; a restated completion had the mirror problem, a second result for one call. The
chip is now opened on **first sight, whichever event arrives first**, and the result is guarded by
its own key, `<id>\0done` — keyed apart because one id has to be able to announce once *and*
resolve once, and a NUL cannot occur in an id. That completion-only shape is not the
unlikely branch but the measured normal one: three real turns on codex-cli 0.153.4 carried **no
`item.started` at all**, every item arriving bare as a single `item.completed`, so a result with
no chip to land in was what happened every time rather than a corner. It is also
what a refusal looks like (`a_real_codex_tool_refusal_is_flagged_on_the_chip`, captured
before the approval was configured), and a refused call the user cannot see is the failure that
arm exists to surface. `codex_mcp_tool_result_reads_content_blocks_and_flags_failure` is the other
pin; both fed a lone `item.completed` and asserted `[ToolResult]`, and both now expect
`[ToolUse, ToolResult]`.
**Antigravity's dedupe had the same asymmetry and now has the same pair.** It matched on
`(Some(ToolUse{..}), Some(i))`, so it suppressed a repeated *announcement* and let a repeated
*result* through — and `TurnState::apply` attaches a loose result to the most recent tool call
still awaiting one. Step 0 opens, step 1 opens, step 0 completes twice, and step 1 — which has
not finished and may never — is shown as complete carrying step 0's output. Both Codex arms in
the same file already guard their result half and key it the same way (`{id}\0done`), and
`seen_tools`' own doc states the rule for *both* per-turn dialects; the guard had followed the
shape the captured turns happen to restate rather than the rule. Low, and the confidence is
medium in one direction only: the captured Antigravity fixtures record only `ACTIVE` being
restated, never `DONE`, so this is the arm matching its sibling rather than a reproduced break.
With the new arm removed, the first chip reads `OUT0-again`.
**The sibling arms had neither guard, which is the same bug twice over.** `command_execution` and
`file_change` were decoded inline while `mcp_tool_call` alone was given `first_sight` on each
half, so a *streamed* step emitted an unconditional `ToolUse` on every `!completed` line — four
`item.*` lines for one shell command opening four chips, three of which never receive a result
and spin for the rest of the turn — and a step whose only line is `item.completed` emitted a
`ToolResult` with no chip to land in, which `TurnState::apply` staples onto the most recent call
still awaiting one: `ls` output, or the contents of a file, landing on whatever `run_query` chip
happened to be open, where the user reads it as the answer to their query. Both arms go through
one `side_effect(id, completed, name, text, is_error)` helper now, which applies the same
`first_sight(id)` on the open and `first_sight("<id>\0done")` on the close. Which of the two
failures fires is not a matter of luck: the completed-only shape is the measured normal case
(above), while `command_execution`'s *own* event shape is the part that stays inferred, which is
why the helper is written to be right for both shapes rather than for the one that was observed.
**Two tests had pinned the unguarded shape and were widened in the same commit** —
`codex_a_side_effecting_item_is_shown_not_swallowed` asserted the single-event form and now
asserts the open/close pair — and one of them is why the arm shipped unguarded at all:
`a_restated_tool_call_does_not_add_a_second_chip` carried a docstring reasoning about
`command_execution` while its body drove `mcp_tool_call`, so the arm the argument was about had
no test whatever. That docstring now names what it drives, and
`codex_a_restated_shell_step_opens_one_chip_and_closes_it` covers the other, driven through
`TurnState` as well as the parser because a chip that never closes is a rendering fact the event
list does not show.
**`ToolUse` and `ToolResult` both carry an `id` now, and it is the pairing key rather than a
convenience.** Every harness puts one on the wire — `tool_use.id` on Claude, `item.id` on
Codex, `step_index` on Antigravity, `callID` on OpenCode, `toolCallId` on Copilot, `call_id` on
Cursor — and without it a result can only be
attached by recency, which is exactly backwards for the ordinary parallel shape: the model
makes two calls in one step, the tools the panel allow-lists being read-only ones
a model routinely batches, and their results arrive in **call** order, so the first result
landed on the second chip and the second on the first. `is_error` travels with the text, so a
query that *failed* read as having succeeded and returned another tool's output, under a green
dot, with the red one on the call that was fine — and `chat::drop_tool_results` keeps
`is_error` deliberately, so a restored conversation showed it permanently the wrong way round.
Sequential calls are unaffected either way, which is why the suite was green: every fixture
opened one call and closed it before the next. `TurnState` keeps a `by_id` side map from call
id to the index of the chip it opened, and it is a **side map rather than a field on
`ToolCall`** because that type is `core::transcript`'s and is persisted to `chats.json` — the
id is plumbing for one turn's pairing and has no meaning in a restored conversation. A `None`
id, or an id no chip was opened under, degrades to the recency rule rather than losing the
result.
**Codex's session model differs in kind, not in field names** — one process per *turn*, resumed
by id (`codex exec resume <id>`), against Claude's one persistent bidirectional process per
conversation. `StreamEvent::SessionStarted { id }` exists to carry the `thread.started`/`init`
id a resume needs; it holds no transcript content and `TurnState` ignores it. The `init` half of
that is Antigravity's, and it still matters now that harness is persistent: its session reads the
id for one purpose only, picking the conversation back up after a Stop that had to kill the
process. Claude's dialect emits no `SessionStarted` at all, having nothing to resume from.
**A side-effecting Codex item is surfaced, never dropped.** `command_execution` and `file_change`
become tool chips even though the read-only sandbox should make them impossible, and the
temptation is to treat them as noise from a path we do not use: rendering them means a
constraint that fails to hold is watched in the transcript rather than found on the filesystem
(`codex_a_side_effecting_item_is_shown_not_swallowed`). Codex `reasoning` items *are* dropped —
the panel has no place for them and Claude's dialect never surfaced them either. And Codex
reports no wall time, so `TurnStats::duration_ms` stays `None` rather than being invented: the
panel's live counter already shows elapsed time, and a fabricated total would disagree with what
the user just watched (`codex_turn_completed_carries_usage_and_no_invented_duration`).
**`codex_stats` reads `input_tokens` and `output_tokens` and adds nothing to them — the opposite
call from the one the OpenCode arm below makes**, which is worth writing down because the two
look like the same question and the wrong inference is one line away. Measured across three
turns on codex-cli 0.153.4, `cached_input_tokens` was 9,984 of 12,595 input, 34,048 of 38,134
and 22,016 of 25,058: always a *fraction of* the input beside it, never a sibling of it. Adding
it (or `reasoning_output_tokens`) would double-count the cache on every turn and overstate a
Codex conversation by roughly the amount the OpenCode fix stopped understating one. It is not
*proven* — Codex publishes no `total` to reconcile against — and it does not need to be, since
with no total the conservative reading is the one that cannot invent tokens the user never
spent. A `total` appearing in a future build settles it.
**Antigravity's `status` is not the verdict, and that is the load-bearing one.** A measured turn
whose only tool call was *refused* still reported `"status":"SUCCESS"` with an empty `response`,
recording the refusal nowhere but in a `denied_actions` array — read on `status` alone, a turn
that did nothing renders as a silent success and the user is left asking why the assistant
ignored them. So the refusal is emitted **as prose**, because the body it would otherwise be
explained in was empty. It is deliberately *not* also `is_error`: that flag is a rendering
decision (below), `status != SUCCESS` is what says the turn produced no answer, and a refused
**side** tool can sit beside an answer that is complete — repainting that answer red and
unformatted is the failure the arm now avoids
(`a_real_denied_antigravity_tool_call_names_the_tool_and_says_so_in_prose`, which asserts the
prose arrives and the answer beside it is left alone).
The rest of the dialect: `conversation_id` sits at the **top level**, not inside `init`, and is
what `SessionStarted` carries here; wall time is `duration_seconds`, a **float**, where every
other dialect reports whole milliseconds or nothing, so `antigravity_stats` **rounds** rather
than truncating (a 2.9985 s turn reads `3.0s`, not `2.9s`); and a tool step is
`step_type: "tool"` with states `ACTIVE`/`DONE`/`ERROR`. **The tool that matters is nested**: an
MCP call arrives as the *built-in* `call_mcp_tool` with the real identity in
`tool_info.parameters.{ServerName,ToolName}`, so the qualified `mcp__schemaic__<tool>` name is
rebuilt — exactly as the Codex dialect rebuilds one from its own `server`/`tool` — because a chip
built from `tool_name` alone labels every database call "call_mcp_tool" and the transcript
cannot tell `run_query` from `propose_table_change`. Built-in tools are reported under their own
names rather than dropped, for the same reason Codex's side-effecting items are, and here it is
not hypothetical: a measured turn ran `list_dir` and `view_file` unprompted, because
Antigravity's filesystem readers are auto-approved in headless mode.
**OpenCode never says a turn is over, and inferring the end is the load-bearing one here.** Every
other dialect emits a terminal event — `turn.completed`, `result`, a final `stream-json` message.
This printer simply stops writing when the session goes idle, so the close is read off
`step_finish.reason`: `tool-calls` means the model is going round again and anything else ends
the turn. It is visible in both directions if it is wrong — end on `tool-calls` and the answer is
cut off before it is written, never end and the app closes the turn on process exit and reports a
working turn as *"ended unexpectedly"*. `an_opencode_turn_ends_only_on_the_terminal_step` holds
the fixture to exactly one `TurnDone`, and it being the last event. The `error` type is the
printer's only failure event and nothing follows it either, so the `TurnDone` comes from there or
not at all (`an_opencode_session_error_ends_the_turn_and_says_why`, which also emits the message
as prose — there is no body it would otherwise be explained in).
**`is_error` is a rendering decision, not a diagnosis, and `opencode_is_failure` is where that
was settled**: it answers true for `"error"` and for nothing else. Its only consumer sets
`core::transcript::Role::Error`, and an error turn renders every `Seg::Text` as plain `text()` in
`theme::error()` and never reaches `render_markdown` — headings and tables come out as raw
`#`/`|`, fenced SQL loses its Insert / Run / Propose bar, and `Seg::Tool` chips are unaffected,
so the bubble ends up half-styled. That is right for a turn with nothing to show and wrong for
one whose answer is real prose that merely stopped short. `length` and `content-filter` are such
turns: the answer is on screen, `opencode_cutoff_note` is the sentence saying the last line was
not the end of it, and flagging them snapped the whole formatted reply to red monochrome with
that advisory sentence rendering its own literal `_underscores_`. The three links —
the decoder's flag, `Role::settled`, `Role::carries_an_answer` — sit in three crates and each was
defensible alone, which is why `an_answer_cut_off_at_the_output_cap_still_renders_as_markdown`
drives the composition, and `a_turn_with_nothing_to_show_is_still_rendered_as_an_error` holds the
other direction so the fix cannot pass by never marking anything.
The rest of the dialect follows from the same shape. **Tokens are reported per *step*, not per
turn** — a measured two-step turn reported input 2497 and then 637 — so `OpenCodeTurn` accumulates
them across `step_finish` events and a footer reading only the last would understate that turn by
a factor of four (`an_opencode_turn_sums_tokens_across_its_steps`); `saw_tokens` keeps "nothing
reported" apart from "zero", so a footer stays blank rather than claiming `0 in / 0 out`.
**And every field the step bills for, which used to be two of four.** Summing `input` and
`output` alone dropped `reasoning` and every cached input token, so `OC_REAL_TOOL_CYCLE`
rendered `↑3.1k ↓12` for a turn that really spent 5,181 — the input understated by 39%, and the
gap widens as cache reads grow. The arithmetic is settled by the fixture rather than by a claim
about OpenCode's source: each step's own `total` reconciles exactly as
`input + output + reasoning + cache.read`, at 2536 and at 2645, and that identity is asserted in
the test so a step whose fields stop adding up fails rather than quietly shifting the footer.
`cache.write` is deliberately left out, because it is not part of that `total`. **Codex's arm
answers the same-looking question the other way** — see `codex_stats` above: OpenCode's four
figures are siblings that sum to a published total, where Codex's `cached_input_tokens` nests
*inside* the `input_tokens` beside it, so one is added and the other must not be. Wall
time is stated nowhere in the stream, but every event carries a millisecond `timestamp`, so the
span between the first and the last is the turn's own and is exact — this is the one dialect
where a duration is *derived* rather than read or left `None`. The session id sits on the **top
level of every event** as `sessionID` rather than in an opening event of its own, so it is taken
from whichever arrives first and announced once; it is what `--session` resumes with. **And the
tool name arrives flattened**: OpenCode folds the server into it, so our own tools come through as
`schemaic_run_query` where Codex keeps `server` and `tool` apart. `opencode_tool_name` rewrites
that one prefix — read off `harness::MCP_SERVER`, because this is the only dialect that has to
decode a name Schemaic itself wrote — back to the qualified `mcp__schemaic__run_query` every other
harness produces, and leaves anything unprefixed alone so a built-in still shows under its own
name rather than being dressed up as an MCP call.
**Copilot's turn ends on the one line shaped unlike the rest, and the line that looks like the
end is not it.** Every event nests its payload under `data`; `result` carries `sessionId` and
`exitCode` at the top level, and it is the only turn-completion event there is.
`assistant.turn_end` closes a single *model call* — the captured turn with two tool calls emits
three, the first before any answer exists — so ending on it would close the panel's turn before
either query ran and land every later event in a turn the app considers finished
(`only_result_ends_a_copilot_turn`). **The session id arrives last rather than first**, and
`result` is the only place it appears, so `SessionStarted` is emitted in the same batch as the
`TurnDone` and *ahead* of it: `ai.rs` scans a batch for the id and then hands it to a pump that
stops at `TurnDone` (`the_session_id_is_emitted_ahead_of_the_turn_closing`). A turn stopped
before `result` therefore leaves no id, and the next one opens a fresh conversation — which
`turn_system` answers correctly by sending the schema again. A non-zero `exitCode` is a failed
turn; an absent one is not, for `opencode_is_failure`'s reasoning. **The turn reports no numbers
rather than wrong ones**: `usage`'s `sessionDurationMs`, `totalApiDurationMs` and
`premiumRequests` are *per session* — measured across a resumed pair, the second turn reported
6,500 ms against the first's 1,992 and two premium requests against one — so a footer reading them
would grow with every question (`a_copilot_turn_reports_no_session_wide_numbers_as_its_own`).
`session.*`, `assistant.turn_start`/`turn_end`, `assistant.reasoning`, `tool_call_delta` and
`user.message` decode to nothing. A fatal error — an unavailable model among them — is not in
this dialect at all: measured, it is plain stderr and exit 1 with no JSON line, so it reaches the
panel through the per-turn loop's not-`ended` arm and `cli_failure_message`, as *"The GitHub
Copilot turn ended unexpectedly: …"* carrying the CLI's own words.
**Cursor's turn has Claude's envelope and not Claude's stream.** `system`/`init` opens it with
the `session_id` — first, as Codex's does, so `SessionStarted` comes before anything else — and
`result` closes it, the only event that does; a `result` whose `subtype` is not `success`, or that
says `is_error`, fails the turn (`the_session_id_arrives_first_and_the_turn_ends_on_result`,
`a_result_that_is_not_success_fails_the_turn`). `thinking` and the echoed `user` line decode to
nothing. **Its numbers are the turn's own, and cached input is input**: `duration_ms` is
per turn — a resumed turn reported less than the first, so it is not cumulative — and `usage`
partitions the prompt, a resumed second turn reporting 287 `inputTokens` beside 24,960
`cacheReadTokens`, which 287 alone cannot be for a prompt carrying the first turn's context. So
`cursor_stats` adds the two, as OpenCode's arm does, and leaves `cacheWriteTokens` out for
OpenCode's reason with less evidence — it was 0 in every measured turn, so whether it is part of
the prompt or a count within it is unknown (`a_cursor_turn_counts_its_cached_input`). A provider
refusal is Copilot's case again: stderr only, with no `result`, so it reaches the panel through
the same not-`ended` arm.
**A Claude turn that fails before the model is called says why only in its `result`, and that
is read now.** Reproduced with an expired login on Claude Code 2.1.289: the CLI writes a
synthetic whole `assistant` message (`"model":"<synthetic>"`, `"error":"authentication_failed"`)
holding *"Failed to authenticate: OAuth session expired and could not be refreshed"*, then a
`result` with `"is_error":true` repeating it in `result` — and streams no `stream_event` delta
at all. `parse_stream_value` takes Claude's prose from the deltas only and reads the whole
message for its tool calls, so the reason was in neither place it looked, and the panel drew an
**empty** reply under `40ms · ↑0 ↓0`, reported as "Claude doesn't respond". The app's fallbacks
could not catch it either: both lines are JSON, so `LineKind::Plain` keeps nothing, and the turn
ended cleanly through `TurnDone`, so the stdout-closed arm, which hands stderr and that prose to
`cli_failure_message`, was never reached. `push_claude` now asks `claude_error_text` — a `result`
whose `is_error` is true, its `result` trimmed and non-empty — and inserts that as a `TextDelta`
**ahead of** the `TurnDone`, so it lands in the turn it explains and is painted in the error
colour. **It keys on `is_error`, not `subtype`**: the captured failure says
`"subtype":"success"`. The `result` is read rather than the synthetic message because it is the
one every failure has. A *successful* `result` is never echoed, because its `result` repeats the
answer already streamed (`a_successful_result_does_not_repeat_the_streamed_answer`).
`claude_spoke` records that the turn has already streamed text, so a reason arriving after a
partial answer is set apart by a blank line rather than run on into its last word
(`an_error_after_streamed_text_is_set_apart_from_it`); it is per turn and cleared with the rest
at the boundary, or a clean first turn would make the second turn's error look like a
continuation (`the_second_turns_error_is_not_set_apart_from_the_first_turns_answer`).
`AUTH_FAIL` is the capture, cut to the fields the parser reads, and
`an_error_result_that_streamed_nothing_says_why` drives it; an error `result` that names nothing
adds nothing (`an_error_result_without_text_adds_none`). Checked live in a sandboxed app, where
the panel now shows the authentication message.
**`push` strips a leading U+FEFF before anything else, because `trim` does not.** A byte-order
mark is not `White_Space`, so a BOM on the first line — what a Windows console redirect or a shim
that re-encodes a pipe prepends — made `from_str` fail and the line was filed as prose. On the
dialects that carry the session id on their *opening* line that costs the whole
conversation's continuity rather than one event: `resume` never learns the id, so every later
turn opens a fresh conversation with no memory of the last
(`a_byte_order_mark_does_not_swallow_the_session_id`, which also checks the line is reported as
JSON rather than kept as a diagnostic).
**`last_line` is the other half of that**, and it exists so a caller need not re-parse: both
session tasks have to tell "no events because this is not JSON" — a fatal error the CLI printed
as prose, and the only diagnostic there will be — from "no events because this is JSON I ignore",
and they were answering it by running `serde_json::from_str` a second time on every line of
every turn. `LineKind::{Blank, Plain, Json}` are the three answers, and JSON this dialect has
nothing to say about is **not** prose: filing it as such put protocol noise in the failure
message (`a_line_reports_what_it_was_without_being_parsed_again`).
**The adversarial-input surface is enumerated rather than sampled.**
`every_harness_ignores_blank_and_malformed_lines` drives sixteen inputs over `Harness::ALL`,
widened from a handful of blanks and one unparseable line: non-object JSON (`[]`, `null`, `0`, a
bare string), a `type` that is null, numeric or an object, a missing, `null` or array `item`,
and a `type` nothing decodes — because a dialect decoder's failure mode is not a panic but a
chip or a `TurnDone` conjured out of a line that meant nothing. The one deliberate exception is now a named test of its own rather than an
absence inside that loop, `an_opencode_step_finish_with_no_readable_reason_ends_the_turn`: this
printer never says a turn is over, so of the two ways to be wrong about a `reason` that cannot be
read, ending the turn truncates one only if a *tool* step ever omits it, while the other stamps
an error on every ordinary turn.
**Which dialects are measured and which are read is the real confidence boundary.** Codex,
Antigravity and OpenCode are pinned by fixtures captured **verbatim** from the installed binaries
(`codex exec --json`, `agy -p --output-format stream-json`,
`opencode run --pure --agent schemaic --format json`) — including a matched Antigravity
pair where the harness and the prompt are identical and permission is the only difference
(`a_real_denied_antigravity_tool_call_names_the_tool_and_says_so_in_prose`,
`a_real_completed_antigravity_tool_call_fills_its_chip_and_succeeds`), which is what makes the
`"status":"SUCCESS"` refusal above evidence rather than a story. `OC_REAL_TOOL_CYCLE` is the
OpenCode one, and it is the fixture carrying the two facts that dialect turns on: a `step_finish`
whose `reason` is `tool-calls` is not the end, and the one whose reason is `stop` is. Each capture also has a second
test driving the same bytes through `TurnState`, so the *composition* is covered and not only the
parser. **That sentence claimed all six and was true of four**, and the two it was not true of
included `AGY_REAL_DENIED_TOOL` — the fixture the whole "SUCCESS with a refusal" argument rests
on. Its response body is empty, so the *only* thing the user reads is the prose this parser
synthesises, and a `TurnState` regression that swallowed that prose would have restored the
silent success with the parser test still green. Both
`a_real_denied_antigravity_tool_call_names_the_tool_and_says_so_in_prose` and
`a_real_codex_tool_cycle_fills_its_chip_and_answers` fold their events through `TurnState` now
and assert on the rendered segments and on the chip — its name, its result, and that it is no
longer spinning — rather than on the event list alone. Keep them byte-for-byte: tidying an id or a usage key turns evidence back into a fixture
that agrees with the code that produced it. The one dialect no binary had ever produced was
Gemini's, and deleting that harness took it with it.
**Copilot's fixture sits one step back from that line, on both counts, and says so.**
`COPILOT_REAL_TURN` is a real 1.0.88 turn — told to call `list_schema` and then `run_query` — but
**trimmed rather than verbatim**: the fields nothing reads (`id`, `parentId`, `timestamp`,
`interactionId`, `apiCallId`, the tool schemas) are cut, every line's shape is otherwise as
written, and the `session.*` lines and all three `assistant.turn_end`s are kept because the
decoder has to walk past them. It is driven through `TurnState` as the others are —
`a_real_copilot_turn_renders_two_filled_chips_then_the_answer` asserts two chips, each resolved
with its own query's answer, and the prose once after them. Everything else in its tests is
hand-written.
**Cursor's is trimmed as well, and in the one place a trim could lie it keeps everything.**
`CURSOR_REAL_TURN` is a real 2026.09.23 turn under `-p --output-format stream-json
--stream-partial-output`, against Schemaic's own `--mcp-serve` over a SQLite endpoint, with most
one-token deltas merged into three per segment, the catalogue lookup's result cut to its opening,
and `hookAdditionalContexts`, `startedAtMs`, `request_id` and the repeated session id dropped.
What it keeps is every field the decoder reads or tells shapes apart by, on every line — in
particular the pre-tool restatement's `timestamp_ms` *and* `model_call_id` and the final
restatement's lack of both, which is the whole of the three-shape rule above; merge those away and
the fixture agrees with whichever rule the code happens to have. It is driven through `TurnState`
too: `a_real_cursor_turn_renders_prose_a_filled_chip_and_prose` asserts one chip — the catalogue
lookup is not one — filled with the server's answer, between the two segments of prose.
**The one shape inside a measured dialect that was itself unmeasured was the `Coalescer`'s, and
it has now been measured.** *Whether Codex restates a message cumulatively* was an assumption
this module is built on rather than something a binary had shown — no captured fixture contained
an `item.updated` line, `CODEX_REAL_TURN`'s answer being one word — and of the two ways that
could be wrong, it turned out to be the first: three real `codex exec --json` turns on codex-cli
0.153.4, of 394, 439 and 209 output tokens with one a ~300-word prose answer, produced **zero
`item.updated` lines and zero `item.started` lines**. Every item, the long answer included,
arrived as a single bare `item.completed` carrying the whole text. So `advance` is called once
per message with no prior text to compare against, `K = 1`, and the Θ(N·K) is Θ(N): the question
of tuning it further is moot on this build rather than answered. **The `Coalescer` is kept
rather than deleted**, which is the trade and not inertia — the alternative is a transcript that
renders every prefix again the day a build starts streaming, a data-loss-shaped bug arriving
from a CLI upgrade with nothing here to catch it, and cumulative restatement is a shape Codex
documents. What the measurement changes is the cost of keeping it (one hash lookup per message)
and the standing of the four tests that drive it: they feed hand-written cumulative strings for
a shape this build never emits, which `the_coalescer_emits_only_what_is_new` now says on its
face rather than leaving a reader to take those lines for a capture. **What the same three turns
do not settle is `command_execution` itself.** Two attempts to make the model run a shell
command came back *"command execution is blocked by this environment's policy"* with **no
`command_execution` item emitted at all** — the refusal happens before an item is created — so
that item's event shape stays inferred, and the arm that turns one into a chip is written to be
correct for the streamed and the completed-only shape alike, so nothing turns on which it is.
`CODEX_REAL_TURN` is confirmed representative rather than a lucky short answer, and **no new
fixture was added**: a 300-word near-duplicate of a shape that one already pins would hold
nothing it does not.
