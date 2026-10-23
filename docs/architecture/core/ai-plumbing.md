# `schemaic-core` — AI prompt + reply plumbing

**AI prompt + reply plumbing** (all pure, all dialect-aware — a prompt that hardcodes
"MySQL/MariaDB" asks a Postgres connection for backtick-quoted SQL the server rejects):
- `prompt.rs` — fences DB content so an embedded ` ``` ` can't escape into prose. Every prompt
  built from server-controlled text goes through it. It also owns **what the assistant is told
  about the result on screen**: `result_shape` renders a `QueryState` as columns + types, row
  count, cap, elapsed, database and (on a failure) the engine's error text — *never a cell
  value* — while `result_attachment` renders the rows the user deliberately attached, capped at
  `ATTACH_ROW_CAP` with the cap stated in its own header so a model handed 200 of 5,000 rows
  can't mistake the sample for the set. **The error text is the one arm gated on `AiData`**, and
  `result_shape` takes the level as an argument for it: an engine's error is not shape.
  `Duplicate entry 'alice@corp.com' for key 'users.email'` is a stored cell quoted back by the
  server. **The gate is `may_query` — `Full` alone — and not `may_attach`**, because the reason
  is level-independent and `Full` is the only level whose consent covers a value the user did
  not hand over: `OnRequest` is the default and its consent line reads *"Rows you attach from a
  result leave this machine with that question"*, and nobody attached that one.
  **What the gate decides is verbatim-or-redacted, not sent-or-withheld.** Below `Full` the
  message goes through `redact_engine_error`, which takes the values out and keeps the
  constraint, the column and the error class, and rides under a note telling the model that
  values were removed and that the user can paste the original line. Withholding the whole
  message was the first answer, and it was right about the value and wrong about the message:
  `users.email` is schema, which every level already sends, and it is the half that makes the
  error actionable — so dropping it bought no privacy and left a model told only *"the last run
  FAILED, the message is withheld"*, the level paying for its caution in usefulness rather than
  in exposure.
  **Schemaic's own note is not the engine's error, and is not gated.** A `Failed` whose source
  is `ErrorSource::App` — a tunnel not up, the statement timeout, a run never sent, a Stop's
  disclosure — is introduced as *Schemaic's own note, not an error from the database engine*
  and sent verbatim at every level, still fenced, since a note can carry a path or a server
  setting. It used to go out as *"The engine's error, verbatim"*, so the model explained a server
  error nobody raised, and below `Full` under the redaction note, telling it values had been
  removed from a sentence that never held any
  (`a_run_schemaic_refused_is_not_called_the_engines_error`). The exemption rests on the source
  being right — it is sound because no `App` note quotes a stored value — which is one more
  reason a producer must not write the server's words as `DbError::Refused`.
  **`redact_engine_error` is per-*family*, not a blunt quote-stripper and no longer
  per-template**, because engines quote identifiers in the same syntax as values (`for key
  'users.email'`, `constraint "users_email_key"`) and blanking every quoted run would destroy
  exactly the half worth keeping. Anchoring each rule on one message's prose failed the other
  way round: `contains("Incorrect ")` missed MySQL 1292's lower-case `Truncated incorrect <T>
  value: '…'` and `contains("invalid input syntax")` named one of PostgreSQL's five
  value-quoting messages, so six live-captured templates reached the model whole. **The two
  rules with a family behind them therefore match on the *shape* the family shares and are
  default-deny** — MySQL's ` value: '…'` (the column form closing on `' for column `, the bare
  form on the line's last quote) and PostgreSQL's trailing `: "…"`, which takes any line whose
  *final* quoted run is introduced by `: `. What keeps the identifiers is that PostgreSQL never
  introduces one that way — `constraint "x"`, `relation "x"`, `column "x"`, `at or near "x"` all
  quote mid-sentence, which `redaction_keeps_the_identifiers_postgres_quotes_the_same_way`
  holds — plus `is_statement_echo`, which exempts the user's own statement outright. Two prose
  anchors remain because they name one message each with no family behind it:
  MySQL/MariaDB's `Duplicate entry 'V' for key` and PostgreSQL's `Failing row contains (V…)`.
  `Key (cols)=(V)` is anchored on **`Key (`** rather than on `)=(`, because `)=(` is also
  ordinary SQL — a row comparison, or any `f(x)=(…)` — and unanchored the rule deleted the very
  clause a syntax error was about (`redaction_leaves_a_row_comparison_in_the_users_own_sql_alone`).
  SQLite's constraint reports name no value and pass
  through untouched. It takes **no dialect** — every engine's patterns are tried, and one
  engine's anchor does not occur in another's message, so a wrong dialect cannot be passed to
  it. It is **not a SQL scan**, so it is not built on `sql::skip_noncode` and is not an
  exception to that invariant: the input is an engine's prose, and the quoting rules are the
  message's rather than the dialect's. Two things are kept whole on purpose, because they are
  the user's own text rather than stored data — a syntax error's quoted-back SQL, and
  PostgreSQL's `LINE n:` echo of the statement with the caret line under it
  (`redaction_keeps_the_postgres_line_echo_of_the_users_own_statement`). That echo and its caret
  line are now exempted **as whole lines, before any rule can bite them**, by
  `is_statement_echo`, and that exemption is precisely what makes the default-deny above safe: a
  statement can end in anything, including `: "…"`. **The rules are applied
  line by line**, so an anchor cannot reach across into the next line's text — and **every
  trailing-phrase rule closes on the *last* anchor in the line, never the first**, in the shared
  `try_redact` (whose `None` for a missing anchor is what lets one rule fall back to a narrower
  one, which `redact_between` could not express). A stored value is attacker-controlled and can
  contain its own closing phrase: `Duplicate entry 'bob' for key 'x' for key 'users.email'`
  closed on the first `' for key ` and shipped `x` to the model
  (`redaction_survives_a_value_holding_the_phrase_that_closes_it`). Closing last can over-redact
  a message that repeats the phrase innocently, or one ending in an unrelated trailing
  parenthetical, which is the safe direction to be wrong in. `redact_trailing_quoted_value` and
  `redact_to_end_of_line` are the two rules with a different shape — a final quoted run, and a
  value that runs to the end of the line (PostgreSQL's `CONTEXT:  JSON data, line 1: {…`).
  **The residual risk is
  named rather than papered over:** a message shaped like nothing in the table passes through,
  so a value in an unrecognised template still reaches the model. What the levels below `Full`
  promise is that *rows* do not leave, and the rules cover the messages that carry them.
  Every string in `redaction_answers_the_messages_the_engines_actually_emit` was **captured from
  a live MariaDB 10.11 and PostgreSQL 16** rather than written from memory of the format,
  because the rules are anchored on exact phrasing and a plausible template no engine emits
  protects nothing — the live capture is what showed that MariaDB spells the column in backticks
  in error 1366, and that PostgreSQL appends the `LINE n:` echo.
  `the_engines_error_leaves_only_where_the_consent_line_covers_it` asserts the property over
  every level rather than one, so a fourth level can't be added on the wrong side, and it
  asserts **both halves** — the value is gone *and* `users.email` survives. The second assertion
  is the point: withholding everything also keeps the value out, so a test checking only the
  value's absence passes just as well against the behaviour this replaced and would not notice a
  regression to sending nothing. Both build on `pipe_table`, the one
  table renderer for anything a model reads (the MCP tools call it too), so a cell containing
  `|`, a newline or a megabyte of JSON is handled identically wherever it appears.
  `ai_fix_prompt` is the **one wording of "fix this"**, for all three ways to ask — the editor's
  error bar, the modal behind its *View*, and the right-click *AI fix* over a squiggle. It
  returns the pair the caller needs (the Ctrl+K input line the user sees and can edit, and the
  intent sent with the SQL), so the three can't drift apart in what they ask the model for; the
  `FixOrigin` is what keeps the editor's *warnings* from being announced to the user as "this
  error". The messages ride in a `fenced` block under `UNTRUSTED_NOTE` like everything else here,
  which the error bar's own `format!` did not: a DB error quotes identifiers and cell values from
  a server that isn't necessarily the user's.
  `explain_error_prompt` is its pair, and the two ask for **opposite things on purpose**: a fix
  lands as a diff in the editor with an Approve behind it, an explanation lands as prose in the
  chat panel, where there is no diff and no gate — so this one says outright not to answer with a
  rewrite, or a reply ending in corrected SQL would invite a copy-paste past every check the fix
  goes through. Its statement is **optional**, which is what lets *Explain* be offered for every
  error the modal can show while *AI fix* is offered for one kind: an explanation needs only the
  words, and a commit error, a failed export or a server that never answered are exactly the ones
  whose modal is otherwise a dead end. **Both prompts take the level and apply `result_shape`'s
  gate to the same text**: below `may_query` the message they carry is the redacted one, while
  the SQL and the statement — the user's own text — still go in full. `ai_fix_prompt` redacts
  the problems **before** anything is built from them, so the `FixPrompt::input` *label* is
  redacted too and not only the intent: the Ctrl+K box shows that label, and a retry sends the
  box's contents, which is the path `editor_pane.rs`'s `CmdK::intent` doc comment already warns
  about. The argument that a button press is itself an election — the same one an attached row
  is, and so grounds for these two being laxer than the text the model is handed *automatically
  each turn* — was weighed again when the redaction landed and **not** adopted: with the value
  gone there is nothing left to widen the gate for, and all three sites keep one rule.
  `the_gate_agrees_with_the_one_result_shape_applies_to_the_same_string` pins that on what they
  *keep* as well as on what they drop, so one of them cannot drift back to withholding the whole
  message while the others redact.
- `summary.rs` — the grid's "AI Summary" cell/column prompts. `sample_column` spreads its
  sample *evenly* across loaded rows rather than taking the first N, because a sorted result's
  head often shares a date/status/prefix that reads as a pattern that isn't real; `sample_row`
  supplies the focused cell's own row, since a lone value rarely explains itself. Samples only
  what is already on screen — no round-trip — so the menu action is instant.
- `seed.rs` — AI-generated seed data (Fill Value / Seed Table). `build_fill_prompt`/
  `build_seed_prompt` assemble the one-shot prompt from the table's DDL plus a *bottom* sample
  of real rows, so enum/format/FK conventions come from data rather than guesswork;
  `parse_fill_response`/`parse_seed_response` read the reply back (fence stripping,
  case-insensitive bare `null`, JSON bool → `"1"`/`"0"` for MySQL `tinyint(1)`).
  `parse_seed_response`'s `SeedError` (`Empty`, `NotJson`, `NotArrayOfObjects`) displays the
  English; the app's Seed rows shows it through `ui::i18n::seed_error_text` rather than
  `to_string()`, the JSON parser's own text inside `NotJson` passing through as it came.
  **Both consent questions are answered inside the builder, not at the call site.** `sample_section`
  takes the `AiData` and drops the rows below `may_query` even when a caller hands them over,
  because the sample was gated on `may_attach` — true at `OnRequest`, the *default*, whose consent
  line reads *"Rows you attach from a result leave this machine with that question"*, and nobody
  attached these. `AiData::Full`'s own variant doc already claimed them (*"and the value samples
  behind AI Fill / Seed"*), and this is the same move `prompt.rs` records for the engine-error
  text. The app's two callbacks do not *fetch* the sample below that level either — a
  `SELECT * … LIMIT 20` the user never ran is a read of their data whether or not it is sent —
  and the builder drops it as well so the decision cannot be lost between the two. When they do
  fetch it, it is on a read-only session whatever the connection (`fetch_query_enforced(…,
  Enforce::ReadOnly)`): the `SELECT` is generated, so no text gate reads it, and the source may be
  a view whose body writes (security review S8; the write-guard invariant, and
  `the_ai_sample_reads_run_on_a_read_only_session` in `main.rs`).
  `schema_section` is the other: `ddl` is an `Option`, `None` at `SchemaScope::None`
  (`main.rs`'s `ai_ddl_for`), where Fill and Seed had been handing the vendor's CLI every column
  name, type, nullability, default and comment of a table whose structure the user had just asked
  to keep back. Both are **withheld out loud**, the shape `ai::render_inline_prompt` already uses,
  because a model told nothing about the omission invents the rest — and the DDL, the rows and
  the row being filled all ride under `prompt::fenced_as` and `UNTRUSTED_NOTE`, with the table
  and column names through `prompt::inline_datum`: a column `COMMENT` carrying a newline and an
  imperative sentence used to land in the prompt's own instruction stream, between *"Fill ONLY
  these columns"* and *"Return ONLY a JSON array"*.
  **`SEED_CELL_CHARS` is the per-cell cap this builder was the only one without**, and it is the
  fourth of them beside `summary::SAMPLE_CHARS` (120), `prompt::CELL_CHARS` (200, for an
  attachment) and `mcp::MCP_CELL_CHARS` (60). It matches the attachment's 200 because a seed
  sample is the same kind of thing — rows the model is asked to imitate — and a sample exists to
  show a column's *shape*, which 200 characters of a `LONGTEXT` does as well as 2 MB. Without it
  `rows_to_json` spliced every cell of twenty whole rows in untruncated, so one 200 KB
  `LONGTEXT`/`JSON`/`MEDIUMBLOB` cell blew `arg_limit()` on the harnesses that take the prompt on
  argv and the spawn was refused — correctly, but with the chat path's wording, naming the schema
  scope and "the query in the editor", neither of which is a lever a grid context menu has. Seed
  and Fill were simply dead on such a table. `clip_cell` keeps newlines where `summary::clip`
  flattens them, these values going into a JSON string where a newline escapes to one token
  rather than breaking a line-oriented list.
  **Clipping the cell narrowed that class and did not close it, because the size is a product of
  three terms and it bounded one.** The app samples twenty whole rows and every column of each
  goes in, so 20 × C × 201 crosses `arg_limit()` — 30,000 UTF-16 units on Windows — at roughly
  **eight** long-text columns, which an ordinary `articles` or `audit_log` table reaches; the
  third term is the DDL, spliced whole, and a wide table with a comment on every column is
  thousands of characters before a row is sampled. So the two aggregates are bounded as well:
  `SEED_SAMPLE_CHARS` (10,000) is what the sampled rows may take *all together* and
  `SEED_DDL_CHARS` (12,000) what the table's definition may. `fitting_rows` keeps as many
  **trailing** rows as fit — trailing because the section says "most recent last" and those are
  what the model is asked to imitate — and keeps **whole** rows, the block being fenced JSON
  where half an object is not an example of the shape to emit; a single row that does not fit
  alone yields none, with the dropped count saying so. `clip_ddl` says so out loud when it bites,
  for `schema_section`'s reason: a model handed a truncated `CREATE TABLE` with no note reads it
  as the whole table and omits the columns it cannot see rather than inventing them.
- `propose.rs` — the AI's proposed table change, as a **patch**: `Proposal`/`ProposedOp`
  deserialize the model's JSON (`{"add_column": {…}}`, externally tagged, `deny_unknown_fields`
  so an invented key fails loudly instead of being dropped), and `apply` lays the ops over
  `TableDraft::from_table` to produce a draft that then goes through `ddl::diff` → `emit` → the
  preview modal like the designer's own. So the model never writes SQL, never writes a
  `ChangeSet` — `ddl::diff` stays the only differ — and the write stays behind the same Apply
  click. **A patch rather than a whole-table draft**, because `diff` compares field by field:
  a model that re-types `varchar(255)` as `VARCHAR(255)` would propose a `MODIFY COLUMN` nobody
  asked for, and one that omitted a column would propose to **drop** it, with nothing
  downstream able to tell either from an intended change. What a patch doesn't name, it doesn't
  touch. Ops compose through the draft's own mutators (`rename_column`, `remove_column`), so a
  rename carries its indexes and keys and a drop cascades to them; the result is run through
  `TableDraft::problems(dialect)` before it is returned, the first refusal coming back as
  `ProposeError::Invalid(TableProblem)`, so the designer's rules stand between the model and the
  preview. Column lookups match case-insensitively — a model routinely writes `Email`
  for `email` — and keep the server's spelling. A foreign key's action is a closed vocabulary,
  since `ddl` writes it straight after `ON DELETE `, and **it is the designer's own list**:
  `is_fk_action` asks `ddl::fk_actions(dialect)`, its `None` entry read as `NO ACTION`. It used
  to be a list of its own, the union of every engine's, so a proposal naming `RESTRICT` on SQL
  Server reached the preview and failed at the server; it is `UnknownAction` now
  (`a_foreign_key_action_the_engine_lacks_is_refused`), as is `SET DEFAULT` on MySQL, which
  InnoDB accepts and never performs.
  **`resolve_target` is the one resolver for *which table* a proposal is about**, and it is one
  function because the two ends of a proposal have to agree: the MCP tool checks the ops against
  a table and answers "Valid. Nothing has run.", and the card then builds the plan the user
  consents to. They read the same JSON by different rules — the tool ignored `proposal.schema`
  while the card required an exact match — so on a database holding both `public.orders` and
  `sales.orders` the model was told its change was valid for one table and the user was offered
  it against the other, and the qualified `sales.orders` form the tool's own description asks for
  dead-ended at the card. The order is the one a caller means: an explicit `schema` field, then a
  qualifier written into `table`, then the bare name — which `DbSchema::find_table` already
  resolves preferring `public`, so the ordinary PostgreSQL case needs no separate default. **It
  refuses anything that is not a base table**, which is the other half of being one function:
  `DbSchema::tables` holds views too, every earlier caller of these lookups came from a tree row
  whose kind the user had already seen, and a view laid under `TableDraft::from_table` emits
  `ALTER TABLE` — which PostgreSQL *accepts* for a rename, under a modal that says "Rename the
  table". The test is `shape() != TableShape::Table` and **not** `is_view`: this is the one gate
  whose whole job is refusing a non-table, and it was asking a two-answer question about an
  object with three, so a MariaDB sequence fell on the "therefore an ordinary table" side, a
  `TableDraft` was built from its eight counter columns, and the plan was `ALTER TABLE sq1 …` on
  a sequence.
  **`apply`'s own guard has to decompose the name the way `resolve_target` does**, which is
  `names_the_same_table`. It compared the raw `proposal.table` to the bare `current.name`, so a
  qualified `sales.orders` resolved through the paragraph above and then dead-ended one line
  later — on exactly the databases with more than one namespace, where the listing prints the
  name that way and `propose_table_change`'s own description asks the model to write it. The
  guard is not dropped, because it still protects the case it was written for: a caller that
  opened a table itself and never resolved anything. So the namespace is enforced **where the
  resolver enforces it** and nowhere else — an explicit `schema` field is an exact lookup
  there, while a qualifier written into `table` is a first attempt the resolver falls back
  from, and on MySQL `mydb.orders` legitimately lands on the unqualified `orders`.
  `FENCE_TAG`/`is_proposal_tag`/`parse` are the *carriage*: a proposal reaches the user as a
  fenced block tagged `schemaic-proposal`, which `ui::markdown` renders as a card. Its own tag
  rather than `json`, because a model discussing a schema prints example JSON constantly and
  none of it is consent to edit a table — an offer has to be something the model made on
  purpose. **End to end:** the model calls the MCP `propose_table_change` to check itself,
  echoes the same JSON into its reply, `markdown::proposal_card` renders it,
  `ddl_preview::preview_proposal` turns it into a plan, and the user clicks Apply. Every step
  before that last one is read-only.
- `transcript.rs` — the rendered shape of one AI turn (`ChatMessage`/`Seg::{Text,Tool}`/
  `TurnNotes`/`TurnStats`), kept here rather than in `schemaic-ai` so the UI crate needn't depend
  on the CLI-integration crate. `ChatMessage::prose` is what conversation replay uses, and copy
  takes it with the turn's notes around it.
  **`elapsed(ms)` is the one turn duration**, a `history::Took` that is `Millis` under a second
  and `Seconds` above and never steps up to `Minutes` as a run's does — shared by
  `TurnStats::summary` (the finished turn's footer) and the AI panel's live counter, which had
  its own copy with a comment promising it formatted "like `TurnStats::summary`'s time part" and
  nothing holding it to that; the two sit in the same place, one replacing the other as the turn
  ends, so a drift would show as the number changing format the moment it stops moving.
  `summary_with(marks, took)` is the composition with the duration worded by the caller and the
  token counts written in `marks`; `summary()` is it with English marks and `Took`'s `Display`.
  **Schemaic's own words in a turn are `TurnNotes`, never a `Seg::Text`** — a `lead` drawn above
  the segments (the no-database-tools note) and a `tail` below them (a spawn refusal, a launch
  that failed, a turn that ended badly, a session refused before it started, the app's
  no-database refusal). They were text segments, so `prose()` returned them and `render_history`
  replayed them into a later session's prompt as `Assistant: …` — the interface's sentences, in
  its language, sent to the model as things it had said. `prose()` does not read `notes`, and
  `history_replay_leaves_out_schemaics_own_notes` (in `app/ai.rs`) pins that with German notes
  around an English answer. **A field and not a new `Seg` variant, on purpose**: `chats.json`
  persists `segs` with no parsing shim, so a variant an older build does not know fails its
  read of the whole file, while a struct field it does not know is skipped — the field is
  `#[serde(default, skip_serializing_if = "TurnNotes::is_empty")]` for the same reader.
  `fingerprint` mixes in both notes' lengths, since a turn gains a lead at its start and a tail
  when it fails without any segment changing.
  **`ToolCall::bare_name` is the one `mcp__schemaic__run_query` → `run_query` derivation**, and
  `short_name` is it asked of a call that already exists. It lives here, below `schemaic-ai`,
  because both answers have to name the same tool: this expression produces the chip label the
  user reads *and* — through `harness::bare_tool_name`, which delegates to it rather than
  re-spelling it — the key Codex approves, the rule Antigravity allows and the list OpenCode
  advertises. The two had been written out separately under a doc arguing against exactly that,
  so a change to the `__` separator would have made the approval grant and the label name
  different tools, with the grant being the half nobody can see.
  A user turn may carry an `Attachment` — the result rows sent with that question. Its
  `summary` persists so a reopened conversation still shows that data went with the turn, but
  the cells are `#[serde(skip)]`: sending the rows was the consent, keeping a client's customers
  in `chat.json` afterwards is a second thing nobody asked for. `retained()` is how a view tells
  a live attachment from a restored one. `total_rows` is the count **before** the cap and is not
  derivable from `rows`: the builder caps on the way in, so a header computed from what survived
  reported "200 of 200" for a 5,000-row selection and handed the model a sample it read as the
  set — the cap notice exists precisely for that case, so the number it needs has to travel.
  `fingerprint` folds the attachment in, per its own rule that a field a mutation can change
  belongs there.
  An assistant or error turn also carries a `harness` — the `Harness::key` of the CLI that
  produced *it*, stamped by `main.rs`'s send path when the turn starts rather than read from the
  live setting at render time, because switching CLI in Settings does not start a new
  conversation. **Which value is stamped is `ai::turn_harness(live, selected, need_new)`, and it
  is the live session's, not the selected one.** `needs_respawn`'s harness term is gated on
  reachability on purpose — choosing a harness that is not installed keeps the working
  conversation rather than trading it for a binary that cannot be spawned — so there is a
  supported state in which the *setting* says Codex and the process answering is Claude.
  Stamping the setting put CODEX over Claude's reply, in the transcript and then in
  `chats.json`, which is the worse of the two failures `harness_label`'s own doc exists to avoid:
  the transcript is then wrong about history rather than merely wrong about now. The selected
  harness is the answer only when one is about to be spawned from it, and the two agree on every
  path except the one the reachability gate creates.
  A `String` and not the `Harness` itself: this crate sits below `schemaic-ai` and
  cannot name that type, so `schemaic_ai::harness::speaker_label` is the only thing that turns it
  back into a name. `#[serde(default)]` is load-bearing rather than habit — every conversation
  already on disk predates the field, and without it the user's saved history stops parsing.
  `ChatMessage::pending` *takes* the harness instead of defaulting it, so the one place a turn
  begins is the one place its author is recorded; `user()` sets `None`, a question having no
  author to record, and so does the app's own "can't reach the database" refusal, which no CLI
  answered. A turn that *fails* keeps its stamp, because the stream flips the already-stamped
  pending message to `Role::Error` rather than pushing a new one.
  **`Role::carries_an_answer` is what `Role::Error` actually decides, and it had no name.** An
  error turn's `Seg::Text`s render as plain `text()` in the error colour and never reach
  `render_markdown`; `ai_panel::render_segments` therefore asks the predicate rather than
  `role == Role::Error`, so the question the view is answering is *does this turn have an answer
  to format* rather than *which variant is it*. `Role::settled(is_error)` is the other half — the
  decoders' flag applied to a turn, only ever raising, so a turn already marked `Error` is not
  un-marked by a later snapshot carrying `false`. The two are here rather than in the view
  because the decoders answer the same question at the far end of the stream
  (`schemaic-ai`'s `stream::opencode_is_failure`) and the link between the two ends is what was
  missing: an OpenCode turn cut off at the model's output cap, and an Antigravity turn whose
  *side* tool was refused, were both flagged, and the whole formatted reply snapped to red
  monochrome — the advisory sentence about the cut-off included, rendering its own literal
  `_underscores_`.
  Also the message box's **prompt recall** (Ctrl+Up/Down): `user_prompts` (the user's own
  questions, newest first, blanks dropped and a repeat kept only at its newest spot) +
  `recall_step`, which is a **cycle** — `None → newest → … → oldest → None` — rather than a
  list that stops at its ends, because the empty box is the only way back out of a recall and
  both keys have to reach it. A cursor past the end of a conversation that has since changed
  reads as `None`, so a stale walk restarts instead of landing somewhere arbitrary. The
  **gating** is `recall_apply` → `Recall::{Refuse, Show}`, here beside the arithmetic rather
  than in the view: it trims, like the send icon and Enter and like `user_prompts` itself, and
  the view's own copy did not — so one space in the box refused the recall silently, the row
  disagreeing with itself about whether anything was typed.
  `regenerate_keep_len` is **Regenerate**'s half of the same territory: how far back to truncate
  before re-asking — every trailing assistant and error turn *and the user prompt they answered*,
  because `ai_send` re-adds that prompt and keeping it would show the question twice. Two turns
  can trail (an answer that streamed and then errored), which is why it is a `rposition` for the
  last user message rather than a peel of one. It answers a **keep-length**, not a drop-count,
  so `Vec::truncate` takes it directly: the drop-count spelling is a `len() - n` at the call
  site, one empty transcript away from an underflow panic. With no user message to re-ask it
  keeps everything rather than clearing the conversation — the caller refuses earlier anyway,
  and a decision that is only safe because of its caller is the composition this file's testing
  rule warns about.
  `ChatMessage::fingerprint` is what the panel's per-message memo compares. The memo held a
  whole `ChatMessage`, so every streamed chunk deep-cloned and deep-compared all N of them —
  over segments that include a tool call's untruncated result — and kept a permanent second
  resident copy of the conversation. It is `O(segments)` and allocates nothing, on the stated
  assumption that a message is only ever *extended*;
  `fingerprint_changes_on_every_mutation_the_stream_makes` walks every mutation the stream
  makes, because a fingerprint that misses one freezes a bubble's content on screen — and it
  enumerated every other field while omitting the one described next, so the arm the paragraph
  below argues hardest for was the arm nothing drove. It covers `harness` now. The
  `harness` is mixed in too, and that is a **deliberate exception** to the assumption above: the
  field is written once at construction and mutated by nothing, so by the rule it need not be
  there at all. It is there because "immutable" is a property of today's call sites rather than
  of the type, and a speaker label frozen on the wrong CLI's name is precisely the bug the field
  was added to fix. **Its bytes, not just its length** — that is the one place the length-only
  rule does not carry, because every other string here only ever *grows* while a harness key is
  swapped whole. Two keys of equal length fingerprinted identically, and a message whose
  fingerprint does not move keeps its rendered speaker label. The first five keys happened to
  have distinct lengths, which was luck rather than design, and **the sixth ended it**: `cursor`
  is six bytes, as `claude` is, so under a length-only fold a bubble re-stamped from one to the
  other would keep the previous CLI's name over the new CLI's answer. A key is a handful of
  bytes, so the cost is still nothing.
  **That property is pinned by a test of its own, and it was written with synthetic keys** —
  `two_harness_keys_of_the_same_length_do_not_fingerprint_alike`, which is separate from the
  walk above rather than one more assertion inside it because, when it was written, **no test
  using real harness keys could catch this**: `claude`/`codex`/`antigravity`/`opencode`/`copilot`
  are 6, 5, 11, 8 and 7 bytes, so a length-only fold passed against all five. It holds `codex`
  apart from `crush` and `cline`, neither of which is a harness, and apart from an absent key and
  an empty one. `cursor` is six bytes, like `claude`, so the shipped pair is now asserted first,
  and the synthetic keys stay for the next harness.
- `chat.rs` — per-connection conversations persisted to `chats.json`. `ChatFile::of` replaces
  every tool `result` with `RESULT_OMITTED` before it reaches disk — a `run_query` result is up
  to 200 rows of real table data, and writing it verbatim exported user data to a plaintext
  config file indefinitely. Stripping happens at the *bytes* boundary, not in `save`, so
  switching connections and back mid-session still shows that session's own results.
  `persistable` drops a still-streaming turn and its unanswered question before capping. A
  restored conversation is transcript, not memory: prose is replayed into a fresh agent-CLI
  session's system prompt, tool calls never are, and neither are Schemaic's own `TurnNotes`
  (above). That session need not be the harness that
  produced the transcript — each message carries its own `harness`, so a conversation restored
  after a switch keeps naming whoever answered each turn while the *next* turn runs on the CLI
  selected now.
  **Prose is not safe merely because it is not a tool result, and `app/ai.rs`'s `render_history`
  is where that is enforced.** At `AiData::Full` the assistant answers "show me the first five
  customers" by writing those rows into its reply, so a replayed turn carries data the tool gate
  would have refused — and a replay happens on exactly the two gestures that change who may see
  it. Lower the level and `needs_respawn` fires on `prev.data != now.data`, so setting the
  connection to `SchemaOnly` and asking anything re-sent those rows under a `tools_line` reading
  *"This connection sends no data at all"*; below `may_attach` the whole replay is withheld.
  Switch the vendor and it fires on `prev.harness != now.harness`, so Claude → Codex shipped
  Claude's answers, rows included, to OpenAI with no gesture from the user that any data should
  cross; each message's own `harness` is what leaves those turns out, a `None` (written before the
  field existed) being replayed as this session's own, the reading `speaker_label` already takes.
  Both are **withheld out loud** — a follow-up like "and the other one?" stops resolving either
  way, and a model told why can say so instead of inventing an antecedent — and `ai_context` now
  takes the harness key for it, so the level and the vendor are read at the one seam that knows
  both.
