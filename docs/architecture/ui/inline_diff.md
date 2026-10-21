# `ui/inline_diff.rs`

`inline_diff.rs` — the Ctrl+K suggestion rendered **in the editor's own line flow**: the lines
it replaces stay where they are, faded, and the lines it proposes appear directly below them,
pushing the rest of the document down. They are Floem *phantom text* (the facility the editor's
inlay hints use too, below), so **the rope is never touched** — `doc.text()` keeps returning the user's own SQL for the
whole preview, and tab autosave, Ctrl+Enter, live validation, completion and the outline all
keep seeing the buffer the user actually has. Splicing the diff into the rope would have been
far less code and would have put text the user never wrote in front of every one of them.
Two halves over one `RwSignal<Option<InlineView>>` (`InlinePreview`): `InlineDiffDoc`, a
`Document` wrapping the editor's real one that delegates everything except `phantom_text` — so
the rope, the undo history, IME preedit and every edit command are untouched — installed with
`TextEditor::use_doc` just before the `SqlStyling` that paints over it; and `segments`, the row
builder **both** halves go through, so the styling that paints the rows and the document that
emits them cannot disagree about what they hold or where they start (how many rows they *occupy*
is `row_split`'s question, below). **`segments` is the expensive one and only `phantom_text` needs
it** — it re-tokenises and re-allocates the whole suggestion for its colours — so the styling and
the gutter strips ask `block_at` instead, which answers the only three things they wanted: is
there a block here, which side of the line, how long. They run *per visible line, per relayout*,
and were re-highlighting the suggestion several times a frame to learn that. The two must agree
byte for byte, because `row_split` uses `Block::len` as a column into the combined line — one
`\n` per added row plus each row's text, and an **empty row still costs the one byte** of the
space `segments` substitutes for it, which is the one place the cheap length could silently
drift. `a_blocks_length_matches_the_bytes_segments_emits` and
`an_empty_added_line_costs_the_space_it_renders_as` pin that seam.
`set_preview` is the only correct way to set the signal: it also bumps `cache_rev`, because the
rows are baked into the line's cached `TextLayout` (see the Floem gotcha on phantom text for that
and the rest of the list's rules). It **returns early when the view is unchanged**, which is
worth the comparison rather than a micro-optimisation: the bump discards every cached line layout
in the editor, and the states that publish `None` are the common ones — every Escape, Accept,
Reject and Ctrl+K passes through one, almost always with `None` already in place.
`InlineView` is the *two* things a Ctrl+K request puts on that surface, not one.
`Working(Range<usize>)` is the request in flight — the document lines it covers (from
`diff::line_span`, since the request is a byte range) fade to say so, and nothing else happens:
no rows are added and **no band is painted**, because nothing has been proposed yet.
`Plan(InlinePlan, HunkCarries)` is the settled suggestion, whose replaced lines fade *and* get
the band while its own lines arrive as phantom rows. **Its rows are lexed where they land, not as
a text of their own.** `InlineView::settled` — what `cmdk_popup` builds, which is why it now
takes the tab's `dialect` — runs `sql_highlight::carried_lines` once over `new_full`, the buffer
as `diff::inline_splice` says Accept would leave it, and `HunkCarries::of` keeps each hunk's
rows' carries. Lexed standalone, a comment or string opened *above* a hunk — by the document or
by an earlier hunk — never reached its rows, which were drawn as code; only what the hunk itself
opened carried. The hunk's first added row is found in the new buffer at its old spot (`del.start`,
or `anchor` for a `before` block, else `anchor + 1`) shifted by the earlier hunks' added-minus-
removed lines, and the carries are kept **only if the rows are really there**, line ending
aside; otherwise that hunk gets none and lexes standalone as it used to, which is a colour
degraded rather than a colour wrong. It is computed once per plan rather than in `segments`
because `segments` runs per laid-out line, and a whole-document pass there is the same
per-line cost `before_col_of` (below) was written to keep off that path. Test plans built by
hand carry `HunkCarries::default()`; `a_hunks_rows_carry_what_the_document_above_them_opened`
pins a block comment and a string opened above, and a comment opened by an earlier hunk.
The two fade depths differ deliberately — `fade()` returns
0.45 for `Working` and 0.65 for `Plan`: waiting is the stronger dim because the faded text is
all there is to look at, while a replaced line still has to stay readable against the one
proposed under it. Both that depth and the `fades(line)` predicate live on the enum, so
`sql_highlight` applies **one** rule rather than a rule per state; `segments` takes the view and
returns `None` for `Working` through `InlineView::plan()`.
The colour is `sql_highlight`'s half of the same signal. `apply_attr_styles` fades a line the
view says it fades — alpha on the token colours **plus a whole-line span**, or an identifier
`lex_line_with` doesn't colour would stay at full strength in the middle of a faded row — and shifts
the line's spans by `Block::prefix_len()` for the one `before` case; Floem calls that hook
with the line's *pre-phantom* columns and adds the phantom spans afterwards, so an end-of-line
block (every block but that one) needs no adjustment at all. `apply_layout_styles` early-returns
on `Working` for the reason above and otherwise pushes the row bands, `diff_del_bg` behind the
replaced rows and `diff_add_bg` behind the added ones, with **`width: None`** — that is what
makes Floem paint across the whole viewport rather than just behind the glyphs, i.e. what makes
a diff row read as a row. It cannot reach further than that, though: the gutter and the
wrapper's right padding are outside the editor's clipping scroll, so those two ends of every
band are finished from *outside* by `editor_pane::inline_band_runs`. The split is structural,
not duplication.
**Which rows to band is asked of the layout, never counted from the plan** — `row_split`, and
both painters go through it. The plan knows how many *lines* the suggestion adds; what has to be
banded and marked is *rows*, and the two part company the moment a line wraps. `Segments`
therefore carries no row count at all any more: a struct holding an answer to that question was
the standing invitation to get it wrong. `row_split` asks `hit_position` at the phantom's start
column, from the same text layout the glyphs came out of, so it cannot disagree with what is on
screen. The trap it encodes: for an end-of-line block the phantom's first byte is the `\n` that
ends the line's own content, and the layout puts that index at the start of the **next** row, so
`row_of(own_len)` already names the block's first row. Adding one to it — assuming the column
names the row the preceding text *ends* on — pushed every band a row late: without wrap the added
rows fell outside the range and got no band at all, so the whole diff read as deletions, and with
wrap it banded the suggestion's first row as a deletion.
**The arithmetic is `split_rows`, split out from the layout question so it can be tested.** Both
defects this code has shipped — a row count taken from the plan, then that off-by-one — lived in
those few lines, and neither was reachable by a test while exercising them needed a real
`TextLayoutLine`. Nine tests now cover it, `a_start_row_at_the_end_bands_nothing_as_added` being
the shipped off-by-one itself. `row_split` keeps only the part that genuinely needs the layout:
which visual row the block starts on.
**`before_col_of` is `segments`' cost taken off the *column translation* path**, and it is
deliberately a transcription rather than a delegation. `InlineDiffDoc::before_phantom_col`
replaces floem's default, which builds the whole suggestion — an `InlinePlan` clone and a
complete `segments()` re-tokenise — to answer a question about columns and byte lengths;
`before_col_of` takes the `(column, byte length)` of each phantom and never looks at their text
or colour. Every column translation in the editor flows through it, so a wrong answer moves the
caret, and it is kept **byte-for-byte faithful to floem 0.2.0's `PhantomTextLine::before_col`**
including its quirks: the answer is the *last* span that claims the column, not the first, and
a column inside a span collapses to that span's own column. `spans` must arrive in the order
floem would iterate them (`offset_size_iter` — insertion order with a running shift), which is
what the caller assembles. The only test worth having is the differential one,
`before_col_of_agrees_with_floems_own_translation`, running both over a corpus that covers a
block at column 0, a block at end of line, both at once, several phantoms at the *same* column
— which is what a block's own parts are, and the case that lets `Block::len` stand in for the
list — and columns before, inside, at both edges of, and past every span.
**The editor's inline prediction is a second phantom on the same wrapper.** `EditorGhost`
carries the caret `offset` it was made at and the `text`, plus that offset's `line` and byte
`col`, so the per-line question floem asks on every relayout is a comparison rather than a walk
of the rope; `doc_len`, the document's length when it was published, which is what tells an
edit that only typed into it from one that changed what it was predicted for (`typed_through`,
under `completion.rs`); and `kept`, set on the rest of a prediction whose first part Ctrl+→
took or the user typed, so the next recompute keeps it rather than predicting afresh while the
caret and that length both stand (`keeps_ghost`), and it holds the caret against the list at
any word (`ghost_displaces_the_list`, both under `completion.rs`); and `remembered`, set on a ghost `intel::predict_from_history` made rather than `intel::predict`,
which does not take the empty word's place from the list and gives way where it opens
(`ghost_displaces_the_list`, under `completion.rs`). It lives in a
`GhostSignal` that `InlineDiffDoc::new` takes beside the preview.
`phantom_text` draws it at the caret's column as a `PhantomTextKind::Completion` with
`affinity: Some(CursorAffinity::Backward)` — floem's own rule for completion phantoms, which
keeps the caret *before* text it has not been given yet — in `editor_theme().fg` at
`GHOST_ALPHA` (0.45). **Not `theme::placeholder()`**: that was checked in the running app and
was drawn but all but invisible on the current-line band. **A remembered ghost is drawn exactly
as a reasoned one** — there is no visible difference between the two; `remembered` changes
how the ghost behaves against the list, not how it looks. A faint `"  Tab"` /
`"  Tab · remembered"` hint after the ghost was removed (GitHub #10): it added noise to the
line, and Tab taking a completion is a convention developers already know.
**A ghost's `(line, col)` are stale during the edit's own relayout**, so `ghost_on` draws
nothing where the line has no character boundary at `col`, past its end included.
`Document::edit` lays the lines out again synchronously, inside the edit, before the caret moves
and before the effect that drops a stale ghost can run — so for that one layout the coordinates
describe the old text. Alt+↓ swapping a `-- 注释…` line in under a `BY ` ghost at column 22 put
the column inside a CJK character, and floem's layout (`VisualLinesRelative` →
`slice_to_cow`) panicked on the UI thread — reproduced in the running app, and the fix verified
there (`a_ghost_whose_column_now_falls_inside_a_character_is_not_drawn`, red before).
It is not drawn while a Ctrl+K view
is up in either state, nor when its text holds a `\n` — and `has_multiline_phantom` does not
consult the ghost, so that refusal is what keeps its answer true. **The refusal is
`shown_ghost(ghost, preview_up)`, and the keys ask it too**: `ghost_on` draws by it, and the
editor's Tab, Ctrl+→ and Escape take or spend a ghost only where it answers, so what a key
acts on is what is on screen (`a_ghost_behind_a_preview_or_holding_a_line_break_is_not_shown`).
It was `ghost_on`'s alone, and Tab read the slot, taking a ghost hidden behind a preview. Refusing to *draw* one was
not refusing to *offer* it: such a ghost was still published, so Tab inserted it and Escape was
spent on it, and `completion::pick_prediction` now drops it before it reaches the signal. The
refusal here stays, as the one `has_multiline_phantom` rests on. `set_ghost` is `set_preview`'s
twin, `cache_rev` bump and early return included, and the early return matters at least as
much here: `completion::recompute_completions` calls it on every recompute, so without the
comparison an unchanged ghost would discard every cached line layout per keystroke. **The ghost
is inserted, never pushed**, because of the column-order rule (the Floem gotcha on phantom
text): the wrapped document's own IME preedit sits at the same caret column, so `phantom_slot`
puts it after every phantom at or before its column
(`the_ghost_slots_in_after_every_phantom_at_or_before_its_column`), and `before_phantom_col`
inserts the ghost's span at the same slot so the caret's column translation and the drawn text
agree. That span is the ghost's own text length at its column, as the one `PhantomText`
`phantom_text` inserts there — the two must stay in step, since a phantom drawn but not
counted would put every caret column after the ghost out by its length.
**The inlay hints are a third phantom on the same wrapper** (`intel::insert_value_hints`, under
`intel.rs`), held in a `HintSignal` — `RwSignal<Vec<InlayHint>>`, sorted by offset — that
`InlineDiffDoc::new` takes beside the ghost. `phantom_text` draws each as a
`PhantomTextKind::InlayHint` reading `"{label}: "`, in `editor_theme().fg` at `HINT_ALPHA` (0.38,
dimmer than the ghost: a hint is read past, never taken), with `affinity:
Some(CursorAffinity::Forward)`, so a caret at the value's start sits after the label, next to the
text it is about to edit. **Which hints a line draws is `drawn_hints`, and both halves read it**:
the document draws those phantoms and `SqlStyling` moves its colour spans past them (under
`sql_highlight.rs`), and a hint one drew and the other did not would slide every colour after it
off its token. It draws none while a Ctrl+K view is up in either state, which owns the line flow
while it is; under that, the pure `hints_on_line` partitions the sorted set to the line's span
(its content less the line ending) and drops a hint whose column is no longer a character
boundary, for the reason `ghost_on` checks one
(`a_line_takes_only_its_own_hints_at_their_columns`,
`the_doc_draws_hints_on_their_line_and_not_under_a_preview`). Hints are slotted with
`phantom_slot` **before** the ghost is, in `phantom_text` and `before_phantom_col` alike, so the
column translation counts the spans the screen shows in the order it shows them — at a shared
column the ghost lands after the label. `set_hints` publishes only a set that differs, and unlike
`set_ghost` it bumps nothing itself: `editor_pane` bumps `cache_rev` from an effect on the signal
(under `editor_pane.rs`). Squiggles and carets needed no change for any of it — floem's
`points_of_offset` and `line_point_of_line_col` already map a column through phantom text
(`col_after`).
