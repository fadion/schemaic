# `ui/markdown.rs`

`markdown.rs` — AI-chat `render_markdown`/`CodeActions`/`code_block` (pulldown-cmark). Its
prose colour and its font size are both **baked into a text `AttrsList`** rather than read in a
style closure, which an `AttrsList` is not — so this module's correctness depends on
`ai_panel`'s bubble container being keyed on `theme::ui_generation()`; see the themable-colour
invariant for the whole of that bargain. A code
block carries a **standing** 24px header — the language on the left, `Copy` and (for SQL only)
`Insert` / `Run` on the right, as words rather than icons, since a permanent icon row over every
block is noise. The header repeats the block's own `border_radius`: floem does not clip a child
to a rounded parent, so a square-cornered fill would paint over the wrapper's arc.
**`Insert` and `Run` are gated on `settled` as well as on `is_sql`, and that bar was left out of
the gate its own body and the proposal card both take.** While a turn streams, a fenced block is
whatever has arrived so far — pulldown-cmark closes the unterminated fence at end of input, which
is exactly why the body withholds its colouring — so an assistant answering "delete the draft
orders" streams `DELETE FROM orders` and then `WHERE status = 'draft';`, and between the two
chunks **Run** was live on the unqualified DELETE. `run_verdict` answers `Confirm` there, not a
refusal, so the only thing between the click and an emptied table was a confirmation the
assistant's prose had just primed the user to accept. `Copy` stays live: it is the one action
that does not commit to the text being complete. Also
`proposal_card`: a fenced block tagged `core::propose::FENCE_TAG` renders as the AI's **proposed
table change** — the table, the model's own summary line, the change count, and a Review button
that hands the parsed `Proposal` to `CodeActions::propose`. The card is the offer, the DDL
preview is the decision: Review runs nothing, it opens the same modal the designer opens.
Recognition lives here rather than in a scanner of its own because pulldown-cmark is already
parsing the reply and hands over the language tag; `core::propose` owns only the tag
(`is_proposal_tag`) and the JSON step (`parse`), so there is one answer to "is this a proposal".
**Gated on `settled`** (the turn has stopped streaming): pulldown-cmark closes an unterminated
fence at end of input, so mid-stream a proposal would render as a card of half-arrived JSON,
flickering "couldn't read this" on every chunk — until then it is an ordinary code block. A
block that doesn't parse is neither dropped nor hidden: the user is told it couldn't be read
*and* still sees what the model wrote, which is what they need to tell it what went wrong.
**`flush_item` runs at the *start* of `Tag::Heading` and `Tag::BlockQuote`, beside the one
`Tag::Table` already had.** A tight list item's own sentence sits in `runs` when the next block
opens, and without the flush it was swept into that block: a bullet reading *Back up the table
first* immediately ahead of a `### Why this matters` rendered as the single heading line
`Back up the table firstWhy this matters`, bullet gone, and ahead of a blockquote it took the
quote's stripe. At the start rather than at `TagEnd` because by the end the two texts are one
vector and no longer separable.
**Which blocks get a Run button is `code_is_sql`, and it is the only place a model's output
becomes one click from the user's database** — so the tag is authoritative (a ```bash block
holding `DROP TABLE` is not SQL, however it reads) and only an *untagged* block falls back to
`sql_leading_keyword`, a whole-word match on the block's first word. The fallback is
deliberately narrow: a leading comment, an opening parenthesis or an empty block all read as
not-SQL, because an untagged block is the uncommon case and a missing Run button is the safe
way to be wrong. Both are unit-tested, including every keyword the list claims — one quietly
dropped is a Run button that stops appearing with nothing else to notice it.
**`CodeActions::insert` answers `bool`, and *Run* is `insert` then `run`.** The insert refuses
when the focused tab is on a connection that is not the active one (`tabsel::scoped_database`,
under `core::tabsel`), and `run` targets the *active* tab — so running after a refused insert
would execute the block in whatever tab is in front, which is the connection the insert had just
declined to open on. The `bool` exists for that one question; the refusal itself is reported by
the action.
**Its words are the catalogue's** (`md-*`) — a code block's *CODE* label when
it names no language (`md-code-block`; *SQL*, and a named language upper-cased, stay as they
are), its Copy/Insert/Run links (`code_action_link` takes a `String`), and the proposal card's
title (`$table`), its change count (`md-proposal-count`, a plural on `$count` in place of
`text::plural`), *Review change…* (`card_button` takes a `String`) and the unreadable line
(`$error`). The model's own text — the reply, the proposal's summary — is never translated;
the unreadable line's `$error` is serde's, and the error `CodeActions::propose` hands back is
shown as it comes — already worded, by `ddl_preview::propose_error_text` or the panel's own
`propose-no-database`/`propose-other-connection`; a code block's Run, refused for the same two
`TabScope`s, says `ai-run-no-database`/`ai-run-other-connection`.
`the_proposal_card_reads_as_it_did_in_english` pins the card's lines.
