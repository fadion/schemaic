# The results strip (and keeping a result)

**Every result is a `ResultPanel`, and the strip that lists them is on screen from the first run.**
A tab holds `result_tabs: RwSignal<Vec<ResultPanel>>` and `active_result: RwSignal<u64>` — a panel
**id**, never an index — and a tab with nothing run yet holds one `Idle` panel, so the strip is
never a bar with no chips in it. One run, one panel; a Run Everything batch, one per statement.
There is no longer a "single-grid path" beside a "batch path": `results_view` and the tab's
`results` signal are gone, and what replaced both is `results_multi`, which every result goes
through.
**Before the first run there is no bar at all** — `Tab::results_untouched` hides it with `s.hide()`,
because a strip holding one chip that says nothing, over a pane that says "Run a query", is two
pieces of furniture for one empty state and 28px off the grid for the rest of the session. It
appears with the run rather than with its rows (a `Running` panel is already not untouched) and goes
again only where the pane is empty for the same reason: every result closed, or the tab respawned.
The affordance it carries is still discoverable, because by the time there is anything to pin the
strip is up.

**Pinning is what the strip is for.** `resultsel::after_run` is the rule — a run replaces the
unpinned panels and leaves the pinned ones alone — so a result can be kept across later runs and
compared against them without a second tab and a second execution. That matters most where it is
least recoverable: re-running is the only way to get a result back, and against a table that is
changing, the "before" is simply gone. Right-click a chip for Pin/Unpin, Close, Close other results,
Close all results; the pinned block sits at the front in pin order, a pinned chip shows a pin where
its × would be, and "Close all" spares the pins — the query strip's rules, restated in
`core::resultsel` and applied by `Tab::begin_run` / `set_pinned` / `close_panels`.
**The comparison itself is a menu entry too.** On a chip whose panel is `Loaded` with a result set
(`affected.is_none()` — a statement's *N rows affected* has nothing to compare; a 0-row result is
offered too, unlike the toolbar's menus), `result_chip_menu`
adds, after Pin and Close and behind a separator, one *Compare with <label>* per other such panel
of the tab, the label cut at 40 characters. Whichever of the two ran first is the left side, so
*added* means added since, whichever chip was right-clicked. "Ran first" is `(ran_at, id)`, not
`ResultPanel::ran_at` alone: `ran_at` is whole seconds, so the statements of one Run All tie on
it — and a tie made the right-clicked chip A — while panel ids are handed out in the order panels
are made, unlike the strip's order, which pinning can rearrange. The panels are
read **once, when the menu opens**, and each entry captures its pair of `DiffSide`s — each panel's
`Arc<ResultSet>`, statement and `ran_at`, which the modal's cards show — the opposite of *Close other results* below it, which asks again at the click. The entry sets
`GridCtx::result_diff` and raises *Compare results* (`result_diff_view.rs`, over
`core::result_diff`).

- **A pinned result is frozen, and this is a safety rule rather than a nicety.** `ResultPanel::frozen`
  is asked by exactly two things, and everything else follows from them. It empties the **edit
  model** (the same way a read-only connection does), which stops cell editing, row insert/delete,
  the commit *and* server-side filter/sort in one move, since all four are gated on that model. And
  it makes `GridState::current_statement` return `None`, which withdraws the affordances that
  re-run without going near the edit model — the capped notice's "read all rows", the export
  menu's "All rows" and the pager. The write half is the one that would have cost data: a grid write's `WHERE` is
  the key columns and their **original** values (`model::RowEdit`) with no guard on the columns being
  set, so a commit from a twenty-minute-old snapshot is a well-formed `UPDATE` that overwrites
  whatever landed in between — and the 1-row safety net cannot see it, because exactly one row *is*
  matched. The re-read half would have destroyed the snapshot itself. `run_query_core` asks
  `Tab::shown_frozen` once more before a view re-run, at the funnel, so a stale affordance can't get
  round it. The grid says so where the user is looking when they try to type: a `· kept — read-only`
  note on the toolbar line, because a cell that silently refuses to open reads as a broken grid.
  **The answer is a `Memo` over one panel (`Tab::panel_frozen_memo(id)`)**, and both halves of that
  sentence are a bug that shipped.
  *Not a sampled `bool`*: pinning the result on screen moves neither its id nor its phase, so it does
  **not** rebuild the grid — a flag read in the grid's builder went on saying "not pinned" for as
  long as that grid stayed mounted, leaving the just-pinned result editable, with its row actions and
  its filter bar, until the user switched away and back. Tracked, the edit-model effect recomputes on
  the pin itself, exactly as it already did for `read_only` beside it, and everything gated on that
  model follows.
  *And over **one panel**, not over "whichever panel is shown"*: asked the second way it **crashed the
  app**, on the plainest gesture the feature has — run a query with a pin present. The shown panel
  changes and the grid is unmounted in the same update pass, so a flip re-ran that grid's edit-model
  effect on its way out, which rebuilt the toolbar's `ai_menu` (a `dyn_container` keyed on the model),
  whose new child's style effect read a `GridState` signal whose scope had just been disposed —
  `Option::unwrap()` on a `None` in `floem_reactive::read`. **A view must only subscribe to facts
  about itself**, or a change that unmounts it races the change it is listening for; and a panel that
  has *gone* keeps its last answer rather than reporting `false` on its way out, so closing the shown
  result cannot flip it at teardown either. The `· kept — read-only` note is shown and hidden by
  `s.hide()` for the same reason — the *Floem 0.2 gotchas* rule, and here also the safe construction.
  `Tab::shown_frozen` is the untracked half of the pair, for callers acting *now* inside an event
  handler; anything answering the question while a grid is mounted takes the memo. Vendored Floem
  patch 8 has since added a general guard against this class — a removed view no longer hears a
  queued `State` message in the batch that removed it — but it covers that one route only, so the
  memo stays (*Floem 0.2 gotchas*, *A view must not subscribe…*).
- **What a panel remembers.** `PanelView` — column widths (with the `grid_char_w` they were measured
  against), the client-side sort, the frozen columns, the hidden columns (`hidden_cols`, an index set,
  the panel's for the frozen columns' reason), and the **staged-edit trio** `dirty` /
  `new_rows` / `del_rows` — as **signals in the panel's own child scope**, not fields in the panel
  list: the grid writes a width on every mouse-move of a resize
  drag, and through the list that would clone and re-notify the whole strip each time. `GridState::new`
  seeds from them (length-checked against the column count, so a restore can't leave the header and
  the body disagreeing) and five effects in `grid_view` mirror changes back. Selection is
  deliberately *not* remembered: it is where the user last clicked, not a property of the result.
  Nothing is persisted — a strip is session-only, like the results themselves.
  **The trio is here because it was not.** `GridState::new` created those three with a bare
  `RwSignal::new`, so they belonged to the `results_area` `dyn_container`'s child scope — which any
  change of the active tab disposes. Switching away and back therefore reverted every staged cell,
  dropped every pending row and unmarked every row set for deletion, silently: the *close* path
  stops to ask about unsaved `.sql` text and an open transaction and had no third question for this.
  They are the panel's for the same reason the widths are — they are about the **result**.
  `GridState::new` adopts them from `gctx.panel` and falls back to fresh signals only where there is
  no panel yet (`app_view`'s template, which `results_multi` fills in).
  **They are cleared where the rows are replaced, not where the view is.** The trio is indexed by
  *data row*, so a re-run returning different rows would leave a staged value pointing at whichever
  row landed in that position — worse than losing the edit. So the clear lives on the one writer
  that means "these rows are being replaced", `Tab::bump_panel_load`, calling
  `PanelView::clear_staged`. A fresh run needs no clear (`begin_run` builds new panels, empty by
  construction) and a commit splice deliberately bumps nothing, which is why it keeps its state.
  The clear is right once the rows are replaced, and it is the *re-reads* the user starts from the
  grid — a filter, a sort, *Filter by*, a page, the capped notice's read-more — that must not reach
  it while anything is staged: each refuses instead (`filter::requery`, `filter::page_turns`, and
  the pager's entry under *Data grid*), since every one of them used to land here and drop the
  trio without a word.
  The frozen columns are `frozen_cols`, spelled out because a *frozen column* and a *frozen result* are
  different things one field apart (`gctx.panel.frozen_cols` is a set of indices, `gctx.panel_frozen` is
  whether the result is pinned), and two `.frozen`s in one data path is a wrong-variable bug waiting
  for its reader.
- **The list and the selection move together, under `batch`.** `begin_run`, `close_panels` and
  `set_pinned` each rewrite `result_tabs` *and* `active_result`; separately, the moment between them
  has `active_result` naming a panel the list no longer holds, `shown_panel` falls through to its
  first-panel fallback, and the body builds that panel's grid in full — `init_widths` sampling 200
  rows across every column, `analyze_edit`, `column_editors` — only to throw it away when the
  selection lands. With a pinned result present that was every run. `floem_reactive::batch` holds
  the effects until both writes are in.
- **A bar describes the result it ran on, so switching results clears it.** `view_err`, `commit_err`
  and `commit_note` are tab-level and are cleared by the grid actions that supersede them (a new
  commit, a fresh filter re-run, a click on the filter bar) — none of which switching results is, so
  a bad `WHERE` typed on the live result kept its red bar over the pinned snapshot the user then
  clicked, and a failed commit followed them onto a result that cannot be committed at all. An
  effect beside the one that closes the find/goto/selection bars clears the three on a change of
  shown panel. `commit_wait` is deliberately exempt: it stands for a write still in flight and
  carries the one-click Rollback, so it belongs to the connection rather than to whatever result is
  on top of it.
- **The body's key is `(panel id, phase, load_gen)`, a deduped `Memo`.** The id makes switching
  results a remount (two panels can both be `Loaded` and be different results); the phase is what
  keeps an in-place commit splice from being one — it replaces the panel's `Arc` without touching
  the id or the nonce, so the grid, its scroll and its selection survive, which is the whole point of
  the splice. `sync_canonical` is pointed at *that panel*, so a splice lands on the result it came
  from. A run passes through `Running` and a filter re-run bumps `load_gen`; both rebuild, which is
  what they mean.
- **Ids, not indices, all the way through.** Panels are pinned (which reorders), closed (which
  renumbers) and replaced, so `shown_panel` finds by id with a first-panel fallback, the strip's
  `dyn_stack` is keyed on the id, and a batch writes each statement's result back by id — a
  positional write would land a statement's result on somebody else's panel when a pin reordered the
  strip mid-run. A result whose panel was **closed** while it ran lands nowhere, which is
  deliberately the same answer the run-generation check gives a superseded run.
- **What a chip says.** The statement, previewed (`history::preview`), capped at
  `result_title_avail()` — the chip's width less the 10px inset, the label's margin and the trailing
  glyph. **The cap is on the label, not on the chip**: `max_width` on the row only clips, so the
  text laid itself out at its natural width and ran under the × and out over the next chip, which is
  the arithmetic `tabs::tab_title_avail` pays for the query strip and for the same reason.
  **The chip's hitbox is the view that carries both the style and the listeners**, which is why the
  tooltip goes on the *label* and the inset is `padding_left` on the chip rather than a margin on
  the label: chained after `.tooltip()`, the chip's style painted a wrapper while the handlers sat
  on a text-sized box inside it, and every click outside the words was swallowed (see
  **`.tooltip()` wraps the view** under *Floem 0.2 gotchas*). The query strip is built the same way,
  which is why its tabs never had it. The tooltip carries the full text, the run's
  age (`ran_ago`, which scales `ran_at`'s epoch seconds to the millis `i18n::relative_time`
  reads — handed straight over, a two-minute-old result said "just now" for seventeen hours)
  and — for a loaded one — `ResultSet::retained_bytes`.
  The last is what makes "keep this result" an informed choice: a pin holds its columns' arenas and
  one 4-byte offset word per cell, which at the 200,000-row default cap is 40 MB across 50 columns
  before a single character of text. Cheap to *hold* (the panel keeps an `Arc`, so nothing is
  copied), not free to keep.
