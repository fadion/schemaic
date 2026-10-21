# `core/diff.rs`

`diff.rs` — the line-level diff behind the inline-AI (Ctrl+K) preview. `line_diff` is the LCS
pass, one tagged row per displayed line (context / removed / added); `inline_plan` re-addresses
those rows as **document line numbers**, which is what lets the UI draw the suggestion in the
editor's own line flow instead of in a box over it — the renderer needs to know which of the
user's lines to fade and where to hang the new ones, not how to lay out a list. Consecutive
non-`Equal` rows group into one `InlineHunk`; its `del` is the half-open range of document lines
the change removes, and its additions hang off the **last** line it removes, which is what puts
the `+` rows below the `−` rows they replace. A pure insertion falls back to the line above it,
and `before` — the one case the renderer special-cases — is true only for an insertion at the
very top of the buffer, which has no preceding line to anchor to. `line_span` is the companion
for the state *before* there is a plan: the request is captured as a byte range, everything
rendering against the editor surface is keyed on line numbers, and the lines being *worked on*
have to be named the same way the lines being replaced are. Its edge is pinned by
`a_range_ending_on_a_newline_does_not_reach_the_next_line` — the last line is the one holding
the range's **final byte**, not the one holding `end`, so a range stopping just after a newline
covers the line it ended and not the empty one after it. `DiffRow`/`build_diff_rows`/
`DIFF_CONTEXT`, which built the old preview *list*, went with the box that displayed it.
**`inline_plan_summary` is the one line the Ctrl+K verdict bar says about a plan** — `2 hunks ·
3 − / 5 +`, singular for a single hunk, and `No changes suggested` where there is no plan to
count. That sentence covers "nothing was suggested" and "the model is still working" together,
because `InlineView::plan()` answers `None` for both and the bar is only ever on screen once the
request has settled; an *empty* plan is a different thing and keeps its zeroes, which is the
pair a test pins. It lives here rather than in the view that draws it because the view built the
string inside a `dyn_container` builder, which floem 0.2 does not track, so the counts could
freeze beside a diff that had moved on — splitting the sentence out is what let the view's half
be a `label` closure and this half be tested at all (*Floem 0.2 gotchas*, the builder-freeze
entry). Its sentence is the English: the bar itself draws `inline_plan_summary_text` (under
`editor_pane.rs`), the same argument restated in the interface's language. This function is
unchanged and no longer has a caller outside tests: it stays as the English that
`english_says_the_inline_plan_summary_as_core_does` holds the bar's wording to.
**`inline_splice` is one function because the preview and the accept have to agree.** It returns
both the text that will actually go into `full[start..end]` and the whole buffer that results, so
`inline_plan` diffs the same string Accept writes. Computed separately, a CRLF buffer plus an LF
reply — the ordinary case on Windows, where the clipboard hands you CRLF and neither
`clipboard-win` nor floem's editor normalises it — made the plan come back *empty*, because
`str::lines` strips a trailing `\r`: the footer read "No changes suggested" while Accept, gated
on the state rather than on the plan, converted every line ending in the range. So the reply is
brought into the buffer's own ending first, taken from whatever its **first** newline uses; a
buffer with no newline is left alone, there being nothing to disagree with.
