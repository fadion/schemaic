# `ui/ai_panel.rs`

`ai_panel.rs` — AI Assistant panel (`ai_panel`/`message_bubble`/`render_segments`/`tool_chip`/
`assistant_footer`). The two roles are drawn **asymmetrically**, and deliberately: the user's
question is a shrink-wrapped right-aligned bubble on `bubble_user_bg`, while the assistant's turn
has no surface at all — full-width prose on the panel, marked only by a 2px `accent` rule down its
*right* edge, with the cost footer separated by space rather than a rule. That is why there is
no `bubble_claude_bg`: the assistant's text (`bubble_claude_text`) is a foreground with no
paired background, and `contrast.rs` audits it against `bg_panel`. The rule is a setting
(`AiUi::gutter`, persisted as `UiState::ai_gutter`, flipped in AI Settings); the padding that
clears it is read from the same signal, so turning it off gives the reply equal insets instead
of leaving it off-centre. It is also the one AI setting outside `ai_settings_now()`, so flipping
it never respawns the session. Two views carry an attachment: `attachment_chip` sits over the
message box showing what the *next* question will send, with an × that is a real cancel — the
last point before rows leave the machine — and `sent_attachment` sits above a past question
showing what it *did* send, expanding to the exact block the model was given rather than a
re-rendering of the grid, so "what did I send?" is answered by looking. A restored conversation
has the summary without the rows and says so (`Attachment::retained`). The chip's
`dyn_container` keys on the summary alone, never the rows: a key clones what it reads on every
notification. **`sent_attachment`'s expand flag is handed in, not owned by the card** — it lives
in the `dyn_stack` item's scope beside `pop`, because the bubble's own `dyn_container` is keyed
on `(fingerprint, is_last)` and the theme generation, and neither a reply arriving (which flips
`is_last` on the question above it) nor a theme switch is about the card. A view-local
`RwSignal::new(false)` is a *new* signal after each rebuild, so a card the reader had opened to
check what was sent snapped shut the moment the answer landed. That scope outlives the rebuilds
and dies with the message.
**The label over a reply names the harness stamped on *that message*, never the live setting** —
`speaker_label(m.harness.as_deref())`, with "YOU" over the user's own turn. This was a literal
`"CLAUDE"`, and the user who found it drove one conversation through three harnesses (ask Claude,
switch to Antigravity in Settings, ask again, switch to OpenCode, ask again) and was told all
three answers were Claude's — each reply itself correct, each harness reporting its own model
accurately, only the name over it wrong. Reading `ai_harness` at render time is the tempting fix
and the worse bug: it would have corrected the newest label and broken every earlier one,
retroactively attributing Claude's replies to whichever CLI happened to be selected when the
panel was next drawn, so the transcript would be wrong about *history* rather than merely wrong
about now. `switching_harness_does_not_relabel_the_answers_already_given` builds exactly that
three-harness conversation and asserts the labels stay put.
**It is `ai_panel::bubble_speaker_label(m)`, a function `message_bubble` calls, and that matters
because of what the tests were doing.** The two lines were inline in the view and the test
module kept its own `label_of` — the same two lines, copied by hand — so changing the view to
`speaker_label(Some(ai_harness.get().key()))`, which is precisely the tempting fix this
paragraph exists to rule out, left both tests green while the transcript relabelled its history.
`label_of` is now a `use` alias for the real function, so the tests and the view cannot say
different things.
**The panel's own words name the selected harness, or nobody.** The empty state reads
*"`<Harness>` not connected."*: `available` has answered per harness since the picker existed
while that string stayed literal, so Codex selected with no `codex` on `PATH` read "Claude not
connected." and sent the user to check an installation that was not the one in question — the
settings modal was fixed for exactly this and the panel was left behind. Where there is no
harness to hand, the words name none: `markdown.rs`'s parse-failure line says *"The assistant
proposed a change that couldn't be read"*, and the AI settings' gutter toggle describes *"the
assistant's replies"*, because that renderer and that setting are about replies rather than
about any one vendor's.
**Off is said before anything else is.** With `AiUi::enabled` false the empty conversation reads
`ai_off_note()` (*"The AI assistant is off. Pick an Agent CLI in AI settings to turn it on."*)
rather than *"`<Harness>` not connected."*, and the message box is the pure
`input_state_of(enabled, reachable, db_down)` → `InputState::{Live, Off, DbDown, Unreachable}`,
in which **off wins over everything**: with the assistant switched off, a down database or a
missing CLI is not why the box is dead, and naming either sends the user to fix something that
is not the reason (`an_assistant_switched_off_says_so_whatever_else_is_wrong`; the other three
keep their old order). It is one memo over all three inputs, for the reason the pair memo it
replaced was one: two memos would be two keys. **The gear stays live** — the panel's own way into
its settings, and so back on, beside Settings' *AI assistant* section — and the panel toggle and Ctrl+Shift+A stay too, so
the panel is there to say that it is off. The transcript stays on screen, but the last reply's
Regenerate icon does not: it has nothing to send to (the off rule under *Architecture
invariants*).
**The panel's own words are the catalogue's** (`ai-*`): the title, tooltips,
empty-state lines, placeholder and disabled-box notes (`ai_input_disabled` takes a `String`),
*YOU* in `bubble_speaker_label`, and a tool result's *(+N more lines)* cut. What stays English
is not the panel's to translate: the agents' product names and `harness::speaker_label`'s, every
prompt sent to the agent. The off note is `lib.rs`'s `ai_off_note()` (`lib-ai-off-note`), once
the `AI_OFF_NOTE` const, translated there for the panel and the app's AI refusals alike.
The turn's figures are worded too: the finished footer is
`TurnStats::summary_with(i18n::number_marks(), i18n::took)` and the live counter
`i18n::elapsed`, both through `took-*`, so German reads `1,2 s` in each rather than the `1,2s`
a remarked English summary gave.
