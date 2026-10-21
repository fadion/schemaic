# Data grid (results grid)

`grid_view` (in `grid.rs`) is built around `GridState` — a `Copy` bundle of `RwSignal`s created once
per result set and threaded into every cell/handler. It holds column widths, the selection
(`active`/`anchor` in **display** coords so selection stays put visually on sort), the display→data-row
`order`, the value-viewer/freeze/edit toggles, the `dirty` edit map, and `vp`/`scroll_to`/`focus_id`
for keyboard nav. **`dirty`, `new_rows` and `del_rows` are not its own**: they are adopted from the
panel (`PanelView`, under *The results strip*), whose scope survives the tab switch that disposes
this bundle's.

- **Two panes** side by side (`h_stack`): a **frozen pane** (row-number gutter + any frozen
  columns) and a horizontally-scrolling **data pane**. Rebuilt by a `dyn_container` keyed on
  `body_rebuild_key` — sort state, the column layout (the frozen set and the hidden set as one
  `core::edit::ColLayout`, since both repartition the columns between the panes), **how many**
  pending new rows there are, and a
  `key_gen` counter — and it is a **memo, not the bare closure it used to be**. `dyn_container`
  does not dedup (`create_updater`'s `UpdaterEffect::run` calls `on_change` on every notification,
  whether or not the value moved), and `new_rows` takes *shape-preserving* writes: every
  `stage_new`/`stage_new_many`/`stage_new_bytes` patches a value inside a row's map and leaves the
  vector's length alone. So filling a pending row cell by cell — the documented Tab/Enter gesture —
  tore both panes, the header and the `win`/`body_h` memos down per cell and re-ran `compute_order`
  over the whole result: measured 14.3 ms per keystroke at the default 200,000-row cap on a numeric
  sort, *before* the rebuild it sits inside, with each rebuild also republishing `gs.order` and so
  re-firing the selection aggregate and restarting the find-count rescan. A memo compares on
  `PartialEq`, which covers all three layout terms rather than only the length —
  `add_cloned_rows`' batching becomes an optimisation instead of the only defence, and a `sort`,
  `frozen` or `hidden` write landing on the value already there stops rebuilding too
  (`adding_a_row_or_changing_the_sort_still_rebuilds` pins that hiding and showing rebuild and an
  unchanged set does not). `key_gen` is the fourth
  term and the only one that is not about layout: it counts changes to the source table's **key
  map**, which the header cells read by value from inside this container, so a schema arriving
  after the result — ordinary on a large server, where introspection is ten-plus round trips per
  database — otherwise never reached them and the key icons and *Follow relation* were missing for
  the life of the result. A counter and not the map, so the memo compares a `u64` per notification
  instead of a `HashMap`. It is generic in the row type purely so the composition can be tested
  with no window (`body_key_tests`); only `Vec::len` is called.
  **Freeze is per-column, any column, and several at once**: `gs.frozen` is a `BTreeSet` of the
  frozen columns' *absolute* indices, written from the header right-click menu (no toolbar button),
  where *Freeze* **adds** that column to the set and *Unfreeze* removes that one alone. It used to
  hold one index, and Freeze replaced it; why a set rather than a freeze-through-this-column is
  under `core::edit`'s `visual_cols`. The data pane renders
  `data_cols` = `ColLayout::scrolling` (an `Arc<Vec<usize>>`) and the frozen pane's header and
  rows (`frozen_row`) draw the gutter and then `ColLayout::frozen_drawn`, an `Arc<Vec<usize>>` in
  index order; cells keep their
  *absolute* `ci` so selection/resize/sort stay consistent. Frozen pane width = `gutter_w()` plus
  the sum of the frozen columns' widths, and it does not shrink (`flex_shrink(0)`). **There is no
  cap on it, deliberately**: freeze enough wide columns and the data pane is left little or no room,
  which is the user's choice to make rather than the grid's to refuse. What it must not cost is the
  horizontal wheel: the frozen pane forwards both axes to the data pane (the scroll-sync rule
  below), so a pane frozen wide still pans the columns behind it. **Nor the keyboard's vertical
  scroll**: a data pane left no room lays out to width 0, and `scroll_active_into_view` guarded
  both axes on the width, so Arrow-Down past the last visible row, PageDown, Go to row and
  find-next moved the cursor off screen. `scroll_target` asks each axis its own extent — the rows
  the viewport's height, the columns its width (`a_zero_width_viewport_still_scrolls_rows`).
  **A frozen column's header is hatched** — faint 45° `/` lines over the whole cell (`header_cell`'s
  `Hatch`, `text_faint` at 0.22, `FROZEN_HATCH_STEP` apart), the one cue that it is frozen, since the
  cell has no room for another icon and a background tint already means *formatted*. `Hatch` is a
  custom painted view because floem 0.2 has no pattern fill (the ER diagram's `EdgeCanvas` and the
  *Chart* modal's `ChartCanvas` are painted the same way, the second because floem has no
  plotting); it clips to its own rect, reads its
  colour at paint time so a theme switch repaints it, and is absolute, `pointer_events(false)`,
  hidden unless the column is frozen, and stacked under the label and `col_resize_handle`. Painted
  rather than bordered, because a border would take width from the header that its column's values
  do not lose, and the label would stop lining up over them. The line geometry is the pure
  `hatch_lines(w, h, step)` (`hatch_tests`: 45°, a step apart, no stripe-free corner).
  **The frozen block ends in one rule, however many columns are frozen** — `FROZEN_EDGE_W` (2px) in
  `theme::grid_frozen_edge()` (`text_faint` at 0.6), laid **over** the last frozen header's own 1px
  hairline rather than beside it, so the two never read as a 3px pair: a 1px right border on the
  frozen header strip (`frozen_header`) and on a `stack` wrapping `frozen_body`, plus
  `frozen_edge_inner`, an absolute, click-through 1px line just inside that border — over the
  hairline in the header, over the last frozen cell's right edge in the body (body cells draw no
  vertical rule). None when nothing is frozen, the gutter's own hairline being the edge then. Both
  panes widen by the 1px border, so the data header and the data body move right by the same pixel
  and stay aligned — **the edge must be on both or on neither**. The body needs the wrapper because
  floem 0.2 has one border colour per view and the frozen body's own top border is the grid's
  hairline colour. This replaced a dim rail down each
  frozen header's inner edges, which did not read well: a rail per header marked each column, where
  the thing to show is where the frozen block ends.
  The list used to be an inline `(0..ncols).filter(!= frozen)` — the second spelling `core::edit`
  warns about — and is now the same core list scroll-into-view sums over.
  The resulting **draw order** — frozen first, then the rest in index order — is
  `core::edit::visual_cols`, and it is what paste, Ctrl+C and an AI attachment walk (see `core::edit`)
  so they agree with the screen rather than with the index.
  **Hiding is per-column too**, from the same menu: *Hide column* after Freeze/Unfreeze (disabled
  where `ColLayout::may_hide` refuses — the last drawn column), and while any are hidden a *Show
  hidden columns (N)* submenu, one entry per column by name and then *Show all*, because a menu
  closes on a click and one click can only show one. `gs.hidden` is seeded from
  `PanelView::hidden_cols` and mirrored back beside `frozen_cols` — session-only, like the rest of
  the panel. `hide_column` is a wrapper over `ColLayout::hide`, which unfreezes a column it hides,
  since frozen and not drawn is no state anyone asked for and it would come back pinned, and moves
  the active and anchor corners off it through `nearest_shown` — the right neighbour, the one that
  closes over it, else the left (`a_corner_on_a_hidden_column_moves_to_the_nearest_shown`) —
  because an active cell on a hidden column is one the keyboard types into and nobody can see.
  `nav_target` takes the `shown` list
  rather than a column count and steps over a hidden column; Right and Left find the nearest shown
  column either side even from one just hidden, and Home/End/Ctrl+Home/Ctrl+End land on the first
  or last shown (`nav_target_steps_over_a_hidden_column`), as do the first-arrow select and Ctrl+A's
  anchor, those two through `ColLayout::edges` — which is also where the gutter's whole-row
  gestures and *Go to row* take their corners. **No editor opens on a hidden column**: the Tab/Enter hop, `+ Row`
  and a clone ask `ColLayout::next_editable`/`first_editable` (why, under `core::edit`), and behind
  them `start_edit` refuses a hidden column and `commit_edit` *drops* rather than stages an edit on
  one — the backstop, since an editor nobody can see commits `''` over a NULL. That drop is why
  `hide_column` commits an edit open on the column **before** writing the hidden set: it is the
  user's typing, drawn until then. The schema tree's column double-click un-hides the column it
  asks for before selecting it, since a hidden column selected whole is a selection nothing paints
  and a scroll to nowhere. **The gutter's `#` corner becomes an eye and a count while columns are hidden**, with a
  tooltip saying how to get them back and a click that shows all: otherwise nothing on screen says a
  result is short a column, and one quietly missing a column reads as one that never had it. It is
  built inside the body's container because that already rebuilds on every change of the set.
  Hiding does **not** touch sort, widths (a hidden column keeps its width for when it returns) or
  the row editor (*Edit row* still shows every column); nor the whole-result exports, for the
  reason under `core::edit`. What it does reach, besides copy and paste: the selection aggregate's
  whole-row reading (its entry below), the find count, whose effect tracks `hidden` rather than
  recounting only because a rebuild happens to republish `order`
  (`the_find_count_tracks_every_signal_it_reads`), and both *Attach … to chat* labels — a consent
  notice, so they count the drawn columns `GridCells::attached` sends, and a hidden one keeps
  them from saying "whole rows" (`attach_scope`,
  `the_attach_label_counts_only_the_drawn_columns`).
- **⚠️ Scroll-sync rule (cost a hang):** a scroll view must **never both read and write the same
  offset signal** — it re-enters its own layout and hangs the UI thread. Strict one-writer/one-reader:
  the **data pane writes `vscroll`** (`on_scroll`) and reads `gs.scroll_to` (keyboard channel); the
  **frozen pane reads `vscroll`** (its `scroll_to`), has **no `on_scroll`**, and never scrolls itself
  on the wheel — it consumes the event (`Stop`) and forwards the delta into `gs.scroll_to`, so the
  gutter isn't a dead zone under the pointer while the data pane still moves. **All three follower
  panes — the frozen header, the data header and the frozen body — forward through one closure**,
  `forward_wheel` in the body builder, which clamps `x` to the scrolling columns' width less the
  viewport and `y` to the bodies' height less the viewport, and writes `gs.scroll_to` from
  `h_off`/`vscroll`; each reads its pan through `widgets::wheel_pan`, so **both** axes and Shift as
  horizontal. The frozen body used to forward only `y` and ignore Shift, and the frozen header had
  no handler at all, so a horizontal wheel — a tilt wheel, a touchpad's `x`, or Shift+wheel —
  reached the grid only over the data pane: freeze enough columns and the sliver left over was the
  only place it worked, reported as "frozen columns block horizontal scrolling". The data header
  had its own inline copy of the clamp, without the Shift reading; it shares the closure now too.
- **Virtual space under the last row.** Both bodies lay out to `consts::body_scroll_h` — the rows
  plus `min(viewport, rows) − one row` — so the last row can be scrolled up to the top rather than
  sitting on the bottom edge, and the maximum scroll is `rows − one row` whether or not the result
  overflows. It **overrides** the height `virtual_stack` gives itself (`rows × row_h`); a
  `margin_bottom` would not work, because `Scroll::child_size` reads the child's layout size and
  margin falls outside it. Held in a `Memo` beside `win` (recomputes on scroll, notifies only on a
  resize) that **both** panes read: a frozen pane sized to the bare rows would clamp its own
  `scroll_to` a viewport early and drift out of line exactly when the virtual space came into view.
  The wheel forwarding that clamps a vertical offset itself (`forward_wheel`, shared by the headers
  and the frozen body) reads that same memo — clamping to `rows × row_h` stops the wheel early over
  those panes alone. The SQL
  editor gets the identical rule from Floem's `ScrollBeyondLastLine`, which is why `body_scroll_h`
  is one function and not two.
- **Column widths** (`gs.widths`) are estimated from content on load; the header's `col_resize_handle`
  drags to resize (moving-view trick) and double-clicks to auto-fit. Cells read `gs.widths` in
  `.style()` so resize is live. Every cell/header uses `flex_shrink(0)` so the row overflows (enabling
  h-scroll) instead of squeezing. **`cell_chrome_w()` is the one composition of what a cell spends on
  itself** beyond the value — `2 · grid_pad_h() + GRID_CELL_DIVIDER`, the divider counting because it
  is a *border* and so comes out of the content box too. Both estimators reserved a flat `22.0`,
  which is a pixel generous at Normal and short by 6px at 130% and 11px at 160%: **auto-fit clipped
  the value it exists to fit**. `numeric_edit_pad_left` composes the same three terms correctly,
  which is what makes this a composition bug rather than a wrong constant. **A numeric column's
  data cells are the second composition, `data_chrome_w(col)`** — `grid_pad_h() +
  grid_num_pad_right() + GRID_CELL_DIVIDER`, the right side padded 14px rather than 10 — which both
  estimators ask per column: sized as a text cell, an 18-digit `BIGINT` was 4px short of itself, and
  a right-aligned number clips its *leading* digits, so `100000000000000000` read
  `00000000000000000` (`a_numeric_column_is_wide_enough_for_its_own_right_padding`; found driving
  the v0.34.0 chart hand check). **And a digit is wider than the average character**: measured
  7.8px at Normal against `grid_char_w()`'s 7, so with the padding fixed the value still clipped.
  `content_w(col, head, value)` sizes the header at the average advance and a numeric column's
  values at `grid_digit_w()` (8px, scaled).
  **A "character" to both estimators is a display column, not a `char`** (`grid_chars`): an East
  Asian wide or fullwidth character counts two — a full em, about twice the average advance
  `grid_char_w()` stands for — a combining mark none, and a control character one, since the cell
  flattens it to a space. Counting `char`s gave a CJK value half the room it draws in and a
  decomposed `é` two characters' worth; on ASCII the two counts agree, so no Latin column moved
  (`a_wide_character_is_sized_as_two_and_a_mark_as_none`,
  `grid_chars_counts_columns_and_stops_at_its_cap`). The name line, the type line and every sampled
  value go through it, the values capped at 60 on open and 140 on auto-fit, and **the cap bounds the
  walk**, not just the answer — `grid_chars` reads at most `cap` characters, since a run of
  zero-width marks would otherwise be read to its end without ever reaching it (`autofit_width`'s
  comment has what the unbounded spelling cost). It is the `unicode-width` table rather than shaping
  each value with `measure_text_px_at` because shaping would be 200 × columns passes per result
  open and one per row on a divider double-click, and would tie the width tests to the installed
  fonts; the estimate stays an average advance, now one that tells a wide character from a narrow
  one. The crate was already in the lock; only the direct edge is new.
  **And stored widths are carried across a live scale change** (`rescale_widths` + `GridState::widths_at`,
  which holds the `grid_char_w()` the current widths were measured at). They cannot follow the scale
  the way everything else in the grid does, because they are pixels *stored* rather than read in a
  style closure — floem wraps only a `dyn_container`'s key closure in an effect and calls the builder
  outside it, so `init_widths`' `grid_char_w()` read subscribes nothing. Raising the scale with a
  result open grew every cell's font, padding and row height while the columns stayed cut for the old
  one, ellipsized across the board until the statement was re-run or every divider double-clicked;
  lowering it left every column ~1.6× wider than its content. It is an **effect** and deliberately
  not a scale term in the rebuild key: keying the grid would discard the child scope and take the
  selection, the scroll position and every staged edit with it, where this recomputes one signal.
  Every column is carried, **including one the user dragged** — a width chosen to fit that column's
  content should still fit it when the content is 1.6× bigger, so there is no `dragged` set to
  maintain — and `min_col_w()` is applied *after* the ratio, since the floor scales too (48px at
  Normal is under the 77px floor at Huge). A `ratio` that is not positive and finite leaves the
  widths alone: the old measurement beats a column collapsed to the floor.
- **Selection**: click sets `active`+`anchor`; `PointerEnter` while `selecting` extends the range
  (drag-select, no capture); a gutter press selects the whole row — its first to last *drawn*
  column (`ColLayout::edges`, which `model::row_selection` takes) — and arms `row_selecting`, whose
  own `PointerEnter` extends by **rows** (`model::row_range_selection`) — a second flag, because
  sharing `selecting` would collapse a gutter drag to one column the moment it crossed a cell.
  Shift+click in the gutter extends from the current anchor. **A right-click inside the selection
  keeps it**, in the gutter and in the cells alike: collapsing to the clicked cell (which the cell
  menu used to do unconditionally) destroys the very block the menu is about, and the entries then
  describe one cell. Outside it, the press selects first, so a menu never describes something
  invisible. Copy (Ctrl+C / toolbar) emits TSV; a lone cell copies its raw value.
- **The grid's Ctrl+letters are Ctrl and nothing else.** Copy, Paste, Select all, Find and Go to row
  are answered by one pure `grid_ctrl_command(key, modifiers) -> Option<GridCtrl>`, which says
  `None` the moment Shift or Alt is held. The arms it replaced were `ctrl && matches!(s, "a" | "A")`,
  so with a cell focused **Ctrl+Shift+A selected every cell** and the AI panel never toggled: a key
  event goes to the focused view and, consumed, to nobody else, and the window's `KeyDown` handler
  that owns the Global shortcuts was never asked. A Shift variant is a different key in this app;
  declined here, it reaches `grid_key`'s `_ => Continue` and falls through to the window. Alt is out
  because AltGr arrives as Ctrl+Alt on Windows and AltGr+V is a character on many European
  layouts — it pasted. `no_global_ctrl_shift_shortcut_is_taken_by_the_grid` walks every
  `Ctrl+Shift+<letter>` row of the `SHORTCUTS` Global group rather than a list of its own, so a
  Global binding added later is covered the day it lands (it was seen red naming Ctrl+Shift+A);
  `the_grids_ctrl_letters_answer_plain_ctrl_only` pins the other side. Both test the pure function,
  and the bug was at the call site, so **`every_letter_arm_in_grid_key_asks_grid_ctrl_command`**
  reads `grid_key` and fails on any `Key::Character` arm that decides its own modifiers (seen red on
  the old spelling). The function spells its letters as `"x" | "X"` pairs rather than folding case
  first, because that is a form `shortcuts`' `bound_letters` reads and this is the grid's one binding
  site for them — folded, the documentation gate saw no grid letters at all (checked: a probe
  `"q" | "Q"` arm fails `every_bound_letter_is_documented` naming `grid.rs`).
- **Paste (Ctrl+V / the cell menu) stages, it does not write.** Every pasted cell goes through the
  same `GridState::stage`/`stage_new` a typed edit does, so it lands as ordinary green edits and
  the write-back plan, the one-row safety net and Commit/Discard all apply unchanged — a paste is
  a batch of edits the user can still look at and throw away — staged through
  `stage_many`/`stage_new_many`, which are where the revert-to-original and
  blank-cell rules are applied (`stage`/`stage_new` are one-element calls into
  them) and which take **one** signal update for the whole paste: `dirty` and `new_rows` are read
  by the painter and by every derived view, so a per-cell update would invalidate the grid ten
  thousand times for one gesture. **What a blank means in a pending row is the caller's choice
  rather than the function's** (`core::edit::BlankCell` + `pending_cell`): a *typed* clear is
  `UnsetsIt` — clearing a field you typed into is an undo, so the column goes back to unset and the
  `INSERT` omits it, letting the server's default fill it — while a **pasted** blank is
  `IsAValue`, the empty string, because a paste is an assertion about values. That arm did not
  exist: `stage_new_many` applied the typed reading to everything, three lines from a `stage_many`
  that staged `''`, so one block wrote `''` above the pending-row boundary and the *server default*
  below it — the same clipboard cell, two stored values, decided by a line the user cannot see — and
  it discarded a value typed into a pending row whenever the cell pasted over it happened to be
  blank. The parse is
  `core::edit::parse_tsv_block`, **the inverse of `GridCells::tsv` for every cell holding neither a
  tab nor a newline**, which is as exact as an unescaped format gets: split on newlines and
  tabs, no quote interpretation. A CSV-style reader here would be the obvious mistake — the copy
  side emits no quoting, so there is none to undo, and unquoting would silently turn a cell whose
  value genuinely is `"hello"` into `hello`. The cost is real in both directions and only one half
  of it is visible: a cell containing a newline arrives as two rows, which shows in the grid, while
  a cell containing a **tab** arrives as two cells on *one* row, every later column shifted left by
  one, the values that land looking like ordinary neighbouring text and the paste's counters with
  nothing to report because the row count is right. That is what the *copy* side counts — see
  `copy_split_note` below. `plan_paste`
  lays the block over the grid: **one copied cell fills the whole selection** (that is how a column
  gets set to a constant), anything larger keeps **its own** shape from the selection's top-left,
  and everything is clipped to the display rows — pending new rows included, so a paste can fill
  rows the user just added. **It takes the layout, because that is what "extends" means**: a block
  grows into the columns drawn beside the anchor, which under a freeze are not the ones indexed
  beside it (`core::edit::visual_cols`), and the index walk it replaced put the second value of a
  two-wide paste on `email` into a frozen `ssn` at the far left of the screen. The single-value case
  still fills the *selection*, which is what is painted highlighted, so it needs no translation —
  less any hidden column in it, which a block likewise extends across (`core::edit::ColLayout`). What falls outside, what lands on a read-only column, and what lands
  on a row marked for deletion are **counted and reported** in the same bottom bar a commit error
  uses (set *after* staging, since `stage` clears it), because a paste that discarded half a
  spreadsheet looks exactly like one that worked. **Which surface** is
  `core::edit::paste_report`'s to decide, pushed down there so it can be tested: a partial paste is
  a `Notice` — an ordinary success with a caveat, on the ordinary chrome — while a paste that
  landed *nothing* is a `Failed` and earns the red fill. Both were errors, so "Pasted 5 cells,
  skipping 1 in read-only columns." was rendered indistinguishably from a write-back that failed.
  **The view does not show `paste_report`'s words, only follows its decision**: core's sentence is
  English, so `paste_selection` asks `paste_report_shown(counts, skipped_deleted, staged)`, which
  restates the same decision over the same arguments in the interface's language — the
  `grid-paste-*` clauses in core's order, joined with `", "`, the figures through `i18n::count` in
  the interface's marks, and `grid-paste-partial` a plural on the raw `staged`. So the decision
  exists twice, and the test is what keeps the two one: `english_says_every_paste_report_as_core_does`
  (`refusal_wording_tests`) holds it equal to `paste_report` over each skipped kind alone and all
  together, at one and 1,500, against 0, 1 and 2,000 deleted and 0, 1, 3 and 12,345 staged; it
  was seen red with the hyphen of *-cell* dropped. `paste_report` itself is unchanged, its
  arguments plain counts. The counts are snapshotted through `PastePlan::counts` **before** staging drains the cell list,
  which is what stops the report claiming every paste landed nothing. **What landed is counted by
  the caller, after staging**, and handed over as `paste_report`'s `staged`: it used to be derived as
  `planned - skipped_deleted`, which is what the plan *intended*, and staging drops entries the plan
  cannot know about — `stage_many` un-stages a cell pasted back over its own original value (the
  rule itself is `core::edit::staged_cell`, in core with the NULL normalisation that makes it right;
  `stage_many` is a loop over it), and
  `stage_new_many` removes a column whose pasted cell is blank. So pasting a column's own values
  over itself reported `Pasted N cells` while `dirty` gained nothing at all. The two `stage_*_many`
  calls return what they changed and the caller sums them; `PasteCounts::planned` is gone, so no
  reader can rebuild the figure that was wrong. A read-only column is
  skipped **in place**, never
  shifted, which would write one column's values into the next. **One thing is interpreted, and
  exactly one**: the literal `NULL` resolves to SQL NULL (`edit::pasted_value`, applied inside
  `plan_paste`, so `PastePlan::cells` carries `Option<String>` and a value reaches the grid already
  an `Option`). It is what the copy side writes for a NULL cell — stored or staged — so reading it
  as text made a copied nullable column come back with every NULL in it replaced by the *string*
  `NULL`, invisible until the Commit after which `WHERE x IS NULL` no longer matched the row. The
  resolution sits in the plan rather than at the two staging calls because those are the seam
  `BlankCell` exists for: one of the pair got a rule and the other did not, and both compiled. The
  match is **exact** — `null`, `Null`, `NULL ` are text — and *typing* `NULL` into a cell still
  stages the string on every path, which is the escape hatch the ruling leaves and the reason a
  column legitimately holding the word `NULL` can still be written, just not pasted. The two writes
  therefore remain reachable from one keyboard, and `grid::cell_ink` → `CellInk` is what keeps them
  apart on screen: it replaced the painter's `if` chain, and a staged `None` renders in the same
  italic a NULL *original* has always had (white on the same green fill), where the two used to
  share one arm and `middle_name = NULL` was pixel-identical to `middle_name = 'NULL'` right up to
  the write. The italic is deliberately not a new vocabulary — "there is no value here" reads the
  same whether the emptiness is stored or staged. The **copy** side is unchanged: the clipboard
  carries the display text, so a spreadsheet still receives four readable characters. An open
  inline editor takes Ctrl+V back — `paste_selection` returns early while `edit_cell` is set,
  explicitly rather than trusting the text field to swallow the key first, because being wrong
  about the dispatch order costs a block overwrite instead of a caret insertion.
  **Fill down / Fill up are a paste whose clipboard is the selection's own edge row.** The cell
  menu offers them after *Paste* and under its gate (`text_editable`, not a row marked for
  deletion), and only when the right-clicked cell is in a selection of two or more rows — a
  right-click outside the selection collapses it to that cell first, so the menu never offers a
  fill of one row. `fill_selection` asks `core::edit::GridCells::fill_plan` and stages the plan
  the way `paste_selection` does: display rows to data rows through `order`, pending rows through
  `stage_new_many` with `BlankCell::IsAValue`, a cell on a row in `del_rows` skipped and counted
  (`Filled N cells, skipping M in rows marked for deletion.` on `commit_note`), and two signal
  updates for the lot. Unlike `paste_selection` it does not return on an open in-cell editor: it
  commits the edit first, so the value being typed is in `dirty` before the plan reads it — a
  source cell mid-edit fills with what the user is looking at. An empty plan is a `commit_err`
  naming both of its causes — no writable column, or a source that is a new row's unset cell or a
  staged DEFAULT — since `fill_plan` cannot say which. Unlike a paste, a fill does not report the read-only
  columns it skipped: they are the selection's own, not cells a clipboard aimed at them.
- **What a cell *says* is resolved in one place, and it isn't the view.** `copy_selection` and
  `attached_rows` read the signals once into `grid_cells` — a `core::edit::GridCells` borrow over
  `rs`, `order`, `formats`, `dirty` and `new_rows` — and ask it for `tsv(rect, layout)` or
  `attached(rect, cap, layout)` (`GridState::layout_untracked`), both of which emit the selected
  columns in **draw** order (`visual_cols`), hidden ones left out, because the receiver reads them
  left to right; they contain no resolution of their
  own, and `displayed_cell_text` /
  `pending_cell_text` are gone. **A copy also says what the format could not carry**, because it is
  the only step that still can: `tsv_block` returns `edit::TsvBlock { text, split }` — `tsv` is now
  a wrapper that throws the count away — counting, from the same string it writes, the cells holding
  a tab or a newline of their own, and `copy_selection` puts `edit::copy_split_note(split)` — in
  the interface's words, `copy_split_note_text` (`grid-copy-split`) — on `commit_note`, the ordinary note surface rather than the red one, since the copy succeeded. Once
  the block is on the clipboard a separator and a cell's own tab are the same byte, so no paste can
  tell them apart, and a split cell keeps the row count right so the paste's counters stay quiet
  while every later column of that row shifts left by one — green, and one Commit from a real
  `UPDATE`. `grid.rs`'s gate asserts `copy_selection` still calls the helper.
  **The export reaches the same resolution through `exported_rows`**, the only export-side function
  that reads `gs.rs`/`gs.order` raw: it hands them to `grid_cells` and returns `GridCells::exported`'s
  `ResultSet` + order, which `core::export` can walk without knowing an overlay exists. Four sites
  rendered the raw pair before — `render_export` (the Copy ▸ format menu), `export_column_json`,
  `export_column_csv` and `save_export` (the file) — so Ctrl+C wrote the staged value while every
  export wrote the one it replaced, and a pending row appeared in neither, with the export modal
  reporting a clean row count and no caveat. `save_export` takes that resolved snapshot **before**
  the save dialog opens, for the reason the statement beside it is snapshotted: the dialog is modal
  and slow, and an edit typed while it stands open must not change what was asked for. The gate is
  `no_export_path_renders_the_unresolved_result`, which lists every export path — those four,
  `export_menu` (whose `Fetched rows (N)` label counts what the file will hold), the header's
  *SQL IN list* (`export_column_in_list`) and the gutter menu's *Copy as* (`render_rows_at`, and
  `export_values_at` for its *SQL VALUES*) — holds each to one of three resolvers, and holds every resolver, the
  `INSERT` pair `insert_blocks_at`/`insert_blocks_all` included, to `grid_cells`; fixing one of
  four is exactly how this class came back. The resolvers are
  `exported_rows`, its count-only half `exported_row_count` (what `export_menu` needs, with
  nothing built), and `exported_rows_at`, its gesture-narrowed sibling for *Copy as*
  (`render_rows_at`), which reads the raw pair plus the selection rectangle and the column layout.
  **The gate also holds `render_rows_at` to the gesture's scope, not just to a resolver**: it must
  contain `exported_rows_at(gs, pos)` and `insert_blocks_at(gs, pos)` and neither
  `exported_rows(gs)` nor `insert_blocks_all(`, because any resolver passed the first check, and
  `exported_rows(gs)` swapped in copied the whole result — up to the row cap — off a right-click
  on one row with the suite green. `export_values_at`, the same menu's *SQL VALUES*, is held to
  that scope too: `exported_rows_at(gs, pos)` and never `exported_rows(gs)`. It shares `render_order`
  with `render_export`, so a row copy and the whole-result copy cannot render a format differently.
  An `INSERT` reaches it through `render_inserts`, which renders each of `insert_blocks_*`'s blocks
  (see `core::edit`: a pending row's unset cells are left out rather than written `NULL`) and joins
  them — and when nothing is left to insert, a selection covering only a new row's unset cells, it
  says so on `commit_note`, and both menu entries leave the clipboard as it was rather than setting
  it to an empty string. **`render_order` is
  also where a clipboard copy gets its target and its caveat**: an `INSERT` is shaped by
  `export::insert_shape`, so it names the rows' own table rather than the tab's sticky `source`,
  and a JSON or CSV copy puts `export_said` over `withheld_columns`, worded by `i18n::export_note`,
  on `commit_note`, the string
  having nowhere to carry a withheld blob but a `null` that reads as NULL. `save_export` shapes the
  file through `insert_shape` too, **one block at a time**, as `render_order` does. **The saved
  `.sql` file takes `insert_blocks_all` too**, and until it did the clipboard and the file disagreed
  about a pending row's `INSERT`: the file went through `exported_rows`, so the row's unset cells
  were written `NULL` over the server's default. `save_export` now takes `exported_rows` only for
  the other formats; for SQL the first block rides in `ExportRequest`'s `rs`/`order`/`source` and
  the rest in `ExportRequest::more`, and the app writes them with `export::export_insert_blocks`.
  With no block at all — only ＋Rows that set nothing — the order is empty and so is the file, as
  the copy is. The gate asserts both `render_export` and `save_export` contain
  `insert_blocks_all(gs)`.
  The reason is under `core::edit`: the rule went out one source
  short twice in the view, most recently without `format::apply`, so a `Timestamp` column attached
  the epoch integer the cell does not show. The **painter** is the exception and stays one:
  `data_cell`'s content `dyn_container` runs per cell per frame reading the signals one at a time,
  so it is the reference implementation `GridCells::text` is written against — the two must be
  changed together, and neither `grid_cells` nor its callers may become a per-frame path. Those
  reads sit in a `create_memo` rather than in the `dyn_container` key itself, and what the memo
  carries is a `StagedFace` and not a raw `CellEdit`; both halves are load-bearing and the
  `dyn_container` gotcha says why. What the memo and the builder both produce is
  `grid::cell_preview` — one spelling of what the cell shows: the value cut after
  `CELL_PREVIEW_CHARS` (200) and its `\r`/`\n`/`\t` flattened to spaces, cut *before* it flattens so a
  1 MiB value is not copied whole to paint 200 characters (`preview_cut` reads at most 201). **The cut
  lands on a grapheme-cluster boundary** (`grapheme_floor`, over `unicode-segmentation`'s
  `GraphemeCursor`), backing off when the 200th character falls inside a cluster: cut there, a letter
  lost its combining mark and an emoji sequence painted as a fragment of itself. A single cluster
  longer than the whole cap is cut at the cap rather than leaving the cell blank
  (`cell_preview_never_splits_a_letter_from_its_mark`). Ctrl+C
  passes `formatted = false` and an attachment passes `true`, which is the *Copy formatted* entry's
  reason for existing. **How a cell is *weighted* is a decision too**, and it is `grid::cell_ink` →
  `CellInk` (`Staged`, `StagedNull`, `Absent`, `Fk`, `Plain`) rather than an `if` chain inside the
  style closure — the fifth case is why: a staged SQL NULL and a staged four-character `NULL` went
  through one arm and painted identically. A paste no longer produces the second by accident, but a
  *typed* `NULL` still does, so the distinction stays load-bearing (see the paste rules above). It
  takes the staged `CellEdit`, and a staged **blob** weighs as `Staged` rather than `StagedNull`:
  it paints `<n bytes>`, and the italic in this grid means *absence*, where a placeholder stands
  for a value that is there. The green fill is what says it changed. A staged **`DEFAULT`** weighs
  as `StagedNull`, italic like a staged NULL: it paints the keyword, which is not a value the user
  wrote, and the value will be the server's.
- **Every menu this grid raises goes through one door, `GridState::open_menu`, and the door is
  where the liveness guard lives.** Each of them — the toolbar's Copy, Download and AI dropdowns,
  the gutter, header and cell context menus, the in-cell picker's own list — is a `Vec` of `Rc`
  closures over *this grid's* signals parked in `ui.overlay.popup_menu`, which is **window**-scoped.
  Nothing but a pointer-down clears it: not a tab switch, not a re-run, not a commit that re-runs
  the query. So a menu routinely outlives the scope its entries close over, and the click that
  follows is a `get_untracked` on a disposed signal — a panic that takes the window and every other
  tab's uncommitted edits. It is not a millisecond race: type a `WHERE` fragment, press Enter and
  open the Copy menu while the re-run is in flight, or press the commit ✓ on a change carrying an
  insert (`CommitDone::FullReran` re-runs) and open a menu while the write is — `apply_splice`
  puts that window at `innodb_lock_wait_timeout`, 50 s by default and open-ended inside a Manual
  transaction. **Guarding the entries one at a time is what was already going wrong**:
  `GridState::alive` is `rs.try_get_untracked().is_some()`, its own doc records the file having had
  that guard "in five places and missing it in five others", and the eight functions the entries
  call each open with an unguarded read. So `guarded_entries` wraps every action — submenus
  included, to any depth — asking `alive` at **click** time rather than at build time, which is the
  whole point; a dead grid's entry is **inert, not absent**, still drawing and still dismissing the
  menu, because `menu_panel` calls `close` after the action whatever the action did. `alive` is
  passed as a closure so the composition can be tested without a window: a `GridState` cannot be
  built in a `#[test]`, while a disposed `Scope` holding one signal reproduces the identical hazard.
  It replaces the action (and recurses into a submenu's children) **in place**, rather than
  rebuilding each entry field by field, so a field added to `MenuEntry` rides through without being
  named here; the keycap's `shortcut` is the first to rely on it
  (`the_guard_keeps_everything_but_the_action`).
  `popup_channel_gate` keeps the door the only one, matching on the single whitespace-stripped write
  `open_menu` makes — rustfmt wraps it across two lines, and a line-based scan would find no write
  at all and pass while every installer went round it.
- **An in-flight AI fill carries `FillTarget`, resolved at launch — never a display coordinate.**
  `stage_fill` used to take the `(display row, pending index)` the user had selected and re-resolve
  it when the model answered seconds later, and both halves move in the meantime. A sort mid-flight
  rewrites `gs.order` while `GridState` survives, so `gs.order.get(disp)` came back a *different
  data row* and the value was staged there as an ordinary green edit — the purple "generating" wash
  having been on the right row the whole time, because `ai_fill_value` already resolved the data row
  at launch for that and then threw it away. A pending row is worse: **✗ Discard** clears `new_rows`
  and bumps `new_rows_gen`, and the next **+ Row** hands out index 0 again, so a reply carrying
  `pending = Some(0)` overwrote what the user had just typed. That is verbatim the bug
  `new_rows_gen` exists for, and the guard had been written into `ai_seed_rows` and not into the
  sibling reaching the same `stage_new`. So the enum carries a committed row by its **data** index,
  stable under a sort, and a pending row by index *and* the generation it belongs to.
- **Right-click menus** (generic `menu_panel` / `ui.popup_menu`): a header offers flat `Copy name`
  (the name the header shows — the alias where the query gave one) and `Copy qualified name`
  (`database.<display_name(schema, table)>.column`, the schema tree column entry's shape, offered
  only when the column has a `model::ColumnOrigin`), then `Copy values › CSV / JSON / SQL IN list`
  of that column's values (`export_column_csv`/`_json`/`_in_list` — the last copies nothing when
  no non-NULL value is left, `IN ()` being no list, and says on the bar when a binary column is
  why). The two names differ on an aliased base column:
  the origin survives an alias, so the qualified entry is offered there and copies the base
  column's real name (`ColumnOrigin::column`), not the alias. A data cell offers `View`,
  `Edit` (editable
  cells only), a Copy entry whose scope and wording come from `edit::copy_scope` (**Copy** for the
  whole block when the right-click was inside a multi-cell selection, **Copy value** for one cell —
  the same word for three different amounts was the bug), `Set to NULL` (editable **and** nullable —
  stages a `CellEdit::Null`, and it is `editable` it asks, not `text_editable`, because emptying a
  blob writes no text), `Set to DEFAULT` right after it (editable, not on a row marked for
  deletion, and `Column::takes_default` — **not** gated on nullable, since a `NOT NULL` column
  with a default is the common case. On a real row it stages a `CellEdit::Default` through
  `GridState::stage_default`, which re-asks the same gate and, like `stage_bytes`, has no
  revert-to-original rule: the default's value is the server's, so there is nothing to compare it
  against. On a pending row it is offered only once the cell holds something, and it calls
  `GridState::unset_new`, putting the cell back to unset — which already *is* the default before
  the row exists, an `INSERT` leaving that column out; the inline editor opens empty over a staged
  `Default`, there being no value yet to seed it with), and
  `AI summary` (reveals the AI panel, prompts with the column's own table + column for context).
  **That table is the column's, not the tab's**: `source_table(gs, Some(ci))` asks
  `export::column_table` over `gs.rs`, and the chat attachment's " from db.t" label asks
  `source_table(gs, None)` → `result_table`, with the tab's sticky `source` only as their
  fallback. Reading `source` told the model a sample of `SELECT * FROM orders`, rerun in a tab
  opened on `users`, came from `users` (`an_ai_prompt_names_the_results_table_not_the_tabs`, which
  also pins that both column prompts pass `Some(ci)`). AI Fill and AI Seed never had the fault —
  they take their table from the edit model (`model.table(ti)`, `insert_target()`).
  **A foreign key is walked both ways, from real rows only** — a pending row has no committed key
  to navigate by. *Follow relation*, on a cell whose column is in one of the source table's keys
  (Ctrl+click is the same call, `follow_relation`), opens the referenced table at the row the key
  names. *Referenced by* is it run backwards: a submenu after it on any cell, one entry per key in
  the source table's database pointing at that table, labelled `[ns.]table (cols)`
  (`RefBySpec::label`), each opening the child table filtered to the rows that point at this one
  (`open_referencing` → `schema::referencing_target`). An entry is offered only for a row whose
  referenced values `schema::referenced_values` can name — not a NULL, not bytes — so the menu
  holds no entry that would silently do nothing, and the submenu is left off when none remain.
  `build_referenced_by` asks
  `schema::incoming_keys` and keeps a key only where every column it references is in the result
  by provenance (`rs.origin_columns`), as `build_follow_specs` does for Follow's, since otherwise
  the row has no value to filter by. Both lists are rebuilt into `GridState` (`follow`,
  `referenced_by`) by the effect that rebuilds the key map, from the same `source_schema` lookup,
  so both need what the key icons need — a source table with its schema loaded. Both mint
  `RerunRequest::approved` although the `SELECT` is the app's own, and hand off through
  `follow_fk` to `open_table_filtered` — see the write-guard invariant.
  **`Edit binary` / `View binary` is the one entry that opens a cell the grid is not holding**
  (`blob_launch` →
  `open_blob` → `ui.tab_actions.view_blob`, over `core::blob::blob_source`; a **double-click** on
  the cell is the same call, which is why both go through `open_blob` rather than reaching the
  action apiece — the connection they carry is a decision, not a lookup). `blob_launch` returns a
  `BlobLaunch { bref, target, stage }`, and its `label()` picks the word: a cell with somewhere to
  put bytes is *Edit binary*, one without is *View binary*. Two labels for one entry because they
  are two different offers, and the panel behind them differs by a button. **It refuses only when
  there is neither anything to read nor anywhere to write.** A NULL cell and a pending new row used
  to be refusals outright, and they were *read*-side ones: there is nothing committed to fetch, so
  `bref` is `None`, the panel opens `Empty` — and that is exactly where a first file goes. `stage`
  is `None` for a column `EditModel::editable` refuses (an expression, a keyless table) or a row
  marked for deletion. With both `None` there is no entry at all: **every reason it cannot open is a
  reason it is absent rather than dimmed**, because the entry is the only signal the app gives that
  a `<n bytes>` cell has anything behind it, so one that opened a panel saying "nothing here" would
  be worse than no entry. `blob_launch` asks `EditModel::takes_bytes`, so `editable` and
  deliberately not `text_editable` — this
  is the one surface that puts bytes in a cell, and `text_editable` is precisely what keeps
  everything *else* out of a binary column. Where the file lands is `GridState::stage_bytes` /
  `stage_new_bytes`, and neither carries the revert-to-original rule a typed edit gets from
  `edit::staged_cell`: the grid holds a placeholder and not the stored bytes, so there is nothing to
  compare a loaded file against and a load always stages. **Both re-ask the write gate and return
  whether they staged**, because the path from `blob_launch`'s answer to this write runs through a
  file dialog and a worker thread, and the grid is free to move in between. **The gate they re-ask
  is the offer's own**, `EditModel::takes_bytes(ci, row_deleted)`, which the offer and the two sinks
  used to spell for themselves with nothing testing any of the three. Its `del_rows` half is the
  load-bearing one: `toggle_delete` purges a marked row from `dirty`, so a file
  staged afterwards puts it back, and the commit then carries a `RowDelete` *and* a `RowEdit` for one
  row — `GridWrite::plan` runs the deletes first, the update matches nothing, and `one_row_verdict`
  rolls the whole batch back over an edit the user could never see. `stage_new_bytes` passes
  `row_deleted: false` — a pending row has no committed row to mark — and re-checks that
  the pending row is still there, `new_rows` being free to shrink under an open dialog (Discard, a
  commit, a removed skeleton row). The `bool` is what the panel reports — see `blob_view.rs`.
  **Both open with `GridState::alive()`**, because the grid can be gone rather than merely changed:
  the sink is an `Rc` over `GridState` sitting on a *window*-scoped signal, and a re-run, a closed
  panel or a closed tab disposes the grid's scope while the file dialog — floem's `open_file` is a
  plain `std::thread::spawn` with no parent window — is still up. That is the `alive()` half of the
  menu-callback rule in the invariants. The other half is `GridCtx::blob_stage`, which carries the
  window-scoped `BlobUi::stage` signal into the grid so `grid_view`'s `on_cleanup` can take this
  grid's sink back out — and it clears it **only when the installed sink answers `false`**, which is
  what makes an identity-free check right with several grids alive: only a dead grid's cleanup runs,
  and only that grid's sink is dead, so a panel opened from a different, living grid keeps its offer.
  It reads through `try_get_untracked`, the window being able to be closing too.
  The `BlobTarget` it builds also carries
  the column's `EditModel::byte_cap`, which is what lets the panel refuse an oversized file at the
  picker instead of at the commit (see `blob_view.rs`). The **row panel's** per-field *Edit*/*View*
  button is built from this same launcher rather than from a second set of rules — see it below.
  The
  *connection* is deliberately not among the refusals: a result on screen came from one by
  construction, and a sentinel for "no connection" would be a rule nothing else in the app keeps.
  The fetch carries `conn_at_load` rather than the tab's current connection, on the argument the
  export already makes — a blob re-read over a different connection is a different database's row
  with the same key. The
  **gutter** has its own menu (`gutter_menu`) rather than the cell one — `Copy`, a `Copy as`
  submenu, the row actions, and the attach entry — because a row-number click has picked out no
  cell, and offering
  Edit field / Filter by this value there would answer a gesture about rows with actions about a
  column. `Copy as` is the toolbar's Copy menu for just these rows: the same
  `ExportFormat::clipboard_formats()` list (text formats only, for the reason `export.rs`'s entry
  gives), each rendered through `render_rows_at` → `edit::GridCells::exported_rows_at`, so it
  copies the rows on screen in screen order, staged values and pending rows included — and, when
  the click is inside a highlighted block, only the block's columns in drawn order, which is what
  the *Copy* beside it takes; SQL goes through `insert_blocks_at` instead (the export bullet above
  says why). *SQL VALUES* follows the list and is not one of its formats: the same rows as bare
  tuples, through `export_values_at` → `exported_rows_at` (`core::export`'s entry has what it does
  with a pending row's unset cells, and with a binary cell, which it writes as `NULL` and names on
  the bar). *SQL UPDATE* and *SQL WHERE IN* close it — `copy_keyed_sql` →
  `edit::GridCells::updates_at`/`where_in_at`, keyed by the write-back's own `row_key` (`core::edit`'s
  entry has the statement shapes and the refusals) — behind `row_gestures_are_safe`, as Delete and
  Duplicate row are, and a refusal goes on `commit_note` with the clipboard left as it was —
  core's `edit::CopySqlRefusal`, worded by `copy_sql_refusal_text` (`grid-copy-sql-*`,
  `grid-copy-update-*`, `grid-copy-where-in-no-key`, `grid-copy-new-row-no-key`) and held to its
  `message()` in English by `english_says_every_copy_and_row_refusal_as_core_does`. On a
  read-only connection or a pinned result the two keyed copies refuse with **that** reason
  (`edit::withheld_model_reason`, held in `GridState::model_withheld` by the edit-model effect as
  the `Option<CopySqlRefusal>` itself, so it is worded where it is shown):
  the model is emptied there on purpose, and its own refusal blamed a keyed table for having no
  key. The submenu is `copy_as_menu(gs, pos, sel)`, and **the cell menu carries it too**, after
  *Copy* / *Copy formatted*, with the cell's display row as `pos`: the gesture is the highlighted
  block when the cell is inside one of more than one cell, else that cell's whole row. **`sel` is
  taken when the menu is built** — the gutter's selection, and for the cell menu
  `edit::toolbar_copy_block` of it — because the cell's right-click has already collapsed an
  outside selection onto the clicked cell, and the live one-cell selection resolved every format
  to that one column (`the_cell_menus_copy_as_takes_the_row_of_a_lone_cell`; the source gate in
  `no_export_path_renders_the_unresolved_result` pins the call). The gutter menu's row
  actions take **every selected row** (`selected_data_rows` → `set_rows_deleted` /
  `clone_rows`) and count them in the label: the same menu naming five rows in one entry and acting
  on one in the next is how four deletions go missing unnoticed. **`set_rows_deleted` batches, and
  it was the one row action in that pair that did not** — the Del key's handler and
  `add_cloned_rows` both already wrote once for the whole selection while this called
  `toggle_delete` per row, so each row wrote `del_rows` *and* scanned `dirty`, and every mounted
  data cell's style closure tracks `del_rows`. Measured on the locked `floem_reactive 0.2.0` against
  a maximised window's ~1,000 mounted cells: 402.8 ms per-row against 0.48 ms batched at 1,000 rows,
  1.93 s against 0.64 ms at 5,000, 7.87 s against 1.19 ms at 20,000 — linear, so *Delete N rows*
  over a Ctrl+A at the default 200,000-row cap extrapolates to about **79 seconds** of frozen
  window, on the same selection Del handles instantly. The set operation is `mark_rows`, shared with
  that handler so the two cannot drift, and observable behaviour is unchanged. The grid's app
  context (`source`, `db_nodes`, `connections`/`active_conn`, `popup`, `summarize`, `dismiss`, …) is
  bundled in `GridCtx`, threaded `results_section → results_multi → loaded_view → grid_view`,
  then stashed in `GridState` (whose `Rc` callbacks live in `RwSignal<Option<…>>` since it's `Copy`).
- **Menu dismissal**: grid cells consume the pointer-down (drag-select), so the root handler never
  fires inside the grid; cell/header/gutter click handlers call `gs.dismiss` (closes both
  `ui.popup_menu` and `ui.context_menu`, guarded).
- **Row view/edit panel** (`edit_row_panel`, replaced the old single-cell value viewer): the cell
  menu's row entry — *Edit row*, or *View row* where `EditModel::any_editable()` says no column of
  the result can be written — opens an **integrated in-flow bottom strip** (not a popup —
  `border_top` + panel bg, like
  the old viewer; the grid above shrinks) rendering the row as a **structured, per-field editor**, one
  row per column, over `core::rowjson`. Header = `Row {gutter#} · {table}` + a Save (✓) icon (shown
  only when the result has ≥1 writable column, text **or** binary — `any_editable`, below;
  otherwise it's a read-only row viewer) then Close (✕);
  a "Saving…" line shows while a save is in flight. **The entry is named for which of the two it
  will open.** It always read *Edit row*, so over a result with nothing writable — a SQL Server one
  before its write-back (`edit::supports_grid_writes`), a keyless table, a join — the menu promised
  an editor and opened a viewer. The action is the same either way; only the word follows the
  result, and the ✓'s gate asks the same question column by column, so the label and the icon cannot
  disagree (`a_result_says_whether_anything_in_it_is_writable`). The panel's **errors are not
  inline** — they go to
  `commit_err`, the same bottom bar a grid commit failure uses, because an inline message rendered
  under the field it came from was, on a JSON column, nested several scrolls down, so a save that
  didn't happen looked like one that did nothing. The JSON tree's own parse errors go the same way,
  and it keeps a **red outline** on the box meanwhile: the bar says what, the outline says *which
  field* — which is also why that editor still owns the error signal it mirrors into the bar (and
  takes back only its own message). The bar's **View** is offered per `ModalError::worth_viewing`,
  and both of this panel's own refusals — a value `rowjson::update_changes` can't read (a
  `FieldRefusal`, which `commit_row_update` words through `field_refusal_text`,
  `grid-row-not-editable`/`-nullable`, held to its `Display` by the same test as the copies), the
  JSON tree's parse error — are `ModalError::app`, for which that comes down to `text::hides_detail`:
  only when one-lining actually hid something; a parse error repeated verbatim in a modal is a
  button that does nothing. A save the server refused is its `Server` words and gets **View** for
  the modal's *Explain*. The panel is capped at **70% of the results area**
  (`edit_row_max`, measured on resize) and that cap sits on the panel's own column, so the field
  list is what shrinks and scrolls; the grid above drops its `min_height` floor while the panel is
  open, since flexbox honours a min-height over a sibling's size and the two together overflowed the
  area — which is what clipped the last fields out of reach of their own scrollbar.
  A scalar field is an `edit_field` bound straight
  to its `FieldSig::buf` with an explicit NULL toggle; a `json`/`jsonb` column gets the
  `core::jsontree` tree editor instead (click-to-edit leaves, collapsible containers, raw-text
  fallback for invalid JSON).
  **Save commits immediately** (its own path, not the staged `dirty` batch): `flush_fields` →
  `field_state` → `rowjson::update_changes` → `build_row_edits` (one `RowEdit` per base table, WHERE
  key from the *original* row) + a single-row `build_row_refetch` → the existing `CommitFn`; on
  success the row splices in place and the panel closes, on failure the panel stays open and the
  message goes to `commit_err`, the bottom bar, like the panel's other errors (above).
  **`flush_fields` is not optional**: clicking Save doesn't blur the field being typed into (floem
  moves focus on a pointer-down only for a `keyboard_navigable` view), so an editor holding a buffer
  of its own — the JSON tree's open leaf — has to be asked to commit before the write is assembled;
  a field that can't (invalid JSON) stops the write rather than letting the stale value through.
  This is the row panel's counterpart to `commit_grid`'s "flush any open in-cell edit first".
  Read-only fields (expression/`binary`) are shown for context but edits to them are rejected —
  a binary one for the panel's own reason, its `ColSpec::editable` being `text_editable`: every
  field here is an `edit_field`, and a binary column's would hold `<n bytes>`.
  **That used to be the end of it, and it left the row panel blind to blobs**: the field showed the
  byte count and offered nothing at all, so a blob was the one column you had to close this panel to
  touch. `field_row` now takes a `blob: Option<BlobField>` — `type BlobField = (&'static str,
  Rc<dyn Fn()>)`, the word its button reads and what pressing it does — which `edit_row_panel`
  builds from the same `blob_launch` the cell menu uses, so a column that launcher refuses reads
  exactly as it did before: the count, and no button. The word is *Edit* where the cell takes a
  write and *View* where it does not, the cell menu's pair shortened because the row is already
  headed by the column's name and *Edit binary* would say it twice. The button is that column's
  editor, in the place every other column's editor is — and it is `field_mini_btn`, the same
  borderless dim text button *Set NULL* / *Set value* / *Unset* use, rather than the bordered pill it
  first shipped as: the pill was the only one in the panel, so what is the same kind of act read as a
  different *kind* of control. Going through `field_mini_btn` also hands this branch the panel's
  `autofocus`, which the pill dropped — a blob column that is the first editable one takes the caret
  on mount instead of leaving the arrow keys moving the grid's selection behind an open panel, the
  way the NULL branch already does (`row_panel_null_gate`).
  **A blob column has to count toward the ✓ as well**, and `any_editable` is where that was missed.
  It gates the Save icon, and `ColSpec::editable` is `text_editable` — so a row whose only writable
  column is binary (a SQLite `BLOB` in a rowid-keyed table, where the rowid is an implicit key and
  therefore left out of `col_table`) showed the *Edit* button, staged the file, and then offered no
  way to save it. It now also asks `EditModel::editable` across the columns, the same predicate
  `blob_launch` asks, so the two cannot disagree about whether the row has anything to write.
  **`grid::blob_value` draws that field reactively off `gs.dirty`, unlike every other read-only
  field here.** The panel seeds its fields from the stored row and rebuilds only when the row or the
  editors change, which is right for a field nothing but its own input can alter — but this one's
  value can change while the panel stands still, because its button stages into the grid. Without
  the subscription the count stayed at the stored size after a file was loaded, and the row's ✓ then
  wrote bytes the panel had never shown. It resolves the two shapes the cell can hold the way the
  grid painter does — the staged value if there is one, else the stored text — and `value_text` is
  the dim, ellipsized line it shares with `readonly_value`.
  **`commit_row_update` folds this row's staged `CellEdit::Bytes` into its own changes** before
  building the `RowEdit`, so the ✓ writes the file the panel was used to load instead of leaving it
  green in the grid awaiting a second, different Commit. **Only `Bytes`, and only ones no field
  change already covers**: a staged *text* edit on the same row is deliberately not swept in,
  because this panel seeds its fields from the stored result rather than from the dirty map and
  would be writing something it never showed. The bytes are appended *after* the text changes, so
  `edit::row_edit_for`'s sort is what puts the `SET` back in column order —
  `a_row_panels_text_fields_and_a_staged_blob_sort_into_one_statement` pins that composition rather
  than either half of it. **A key column is editable**, here as
  in the grid (`EditModel::editable` asks only whether the
  column maps to a base table) — the single exception being an implicit key, which maps to no column
  of the table and is left out when the model is built — which is why both re-fetches go through
  the one `edit::refetch_key`:
  the `UPDATE` keys on the *original* row, but the re-fetch has to look for the key it just wrote.
  Because this path is separate from the staged batch, its splice un-stages **only its own** changed
  columns (`model::drop_committed`) — a green cell edit elsewhere in the grid is still unwritten.
  State on `GridState`: `edit_row_open` / `edit_row_di` / `edit_row_saving` (errors have no signal
  of their own — see `commit_err` above). Real
  rows only (a pending new row is filled via inline cells); Esc closes it (after the find bar).
- **Inline edit writes back to the DB.** Per-column *provenance*: `schemaic-db` runs on
  **`mysql_async`** (sqlx's MySQL driver discards `org_table`/`org_name`/key flags), so each `Column`
  carries `origin: Option<ColumnOrigin>` — real `database`/`table`/`column` + `ColumnFlags`
  (pk/unique/not_null/auto_increment), or `None` for an expression (read-only). `analyze_edit` builds
  an `EditModel`: which columns are editable + each base table's WHERE key (schema PK first —
  authoritative for composite keys; else a fully-present unique NOT NULL index; else wire PK flags
  when schema isn't loaded; else, last, a backend-asserted implicit key — SQLite's projected
  `rowid` — which keys the write but is itself never editable). Flow: double-click an editable cell (or Enter) → inline editor; **Enter**
  stages into `gs.dirty` and paints the cell `grid_edit_staged()`; **Ctrl+Enter** / toolbar ✓ calls
  `commit_grid` → a `GridWrite { updates, inserts, deletes }` → the app's `commit_edits`. On success
  the app re-runs the query; on failure the error shows in the toolbar and green edits stay. The
  strip's copy icon after the ✓/✗ pair (*Copy the changes as SQL*, `copy_staged_sql`) builds that same
  `GridWrite` after the same flush of an open in-cell edit and puts `edit::staged_sql` of it on the
  clipboard, with the confirmation on `commit_note` — readable SQL with literals, not the bound
  statements the commit runs (see `core::edit`). No global "Edit"
  toggle. A read-only cell's double-click only selects it — **except a binary one**, where it opens
  the binary-cell panel (`open_blob`, the same call the cell menu's entry makes). C2 — now
  `EditModel::text_editable` — keeps the inline editor off that column whether or not the column
  itself is writable, so the gesture was free there, and "open the thing I clicked" is what a
  double-click means everywhere else in the app. **Enter does the same thing now, and it did not
  before**: both gestures read `EditModel::activation` (see `core::edit`), where the key handler
  asked `text_editable` and stopped — so Enter did nothing at all on a blob cell, and the panel,
  which is the grid's only byte-write surface and has no context-menu key binding, was pointer-only.
  `open_cell_panel` is the one spelling of *aim the panel*, called by both so they cannot come to
  differ, and it stays silent when `blob_launch` refuses because that refusal is about the row.
  `row_at` is what the key handler needs to get there: the cell builders are handed a result index
  and a pending index already, while the key handler works in display coordinates and has to split
  one itself — passing `0` as the data index for a pending row, which is what `data_row` passes and
  for the same reason, there being no result row behind one and every consumer reading `pending`
  first.
  **The inline editor is an `edit_field`, not floem's `text_input`, because `text_input` takes no
  IME** — it never allows it on the window and handles neither preedit nor commit — so a cell could
  not take Chinese, Japanese or Korean at all; floem's editor engine, which `edit_field` is built
  on, composes. `FieldCfg::bare` draws it in place: no border, padding, background or rounding, and
  exactly one line tall (`height(line_h())`) for the cell to centre. **Not `height_full`** — the
  editor fills its box, so a box the cell's height would put the line at the cell's top. Every key
  policy is a `FieldCfg` hook, because the editor consumes every key it is sent and nothing bolted on
  from outside would hear one: `on_submit` is Enter (stage and close, handing the grid its keyboard
  back — or, in a pending row, hop on), `on_tab`/`on_shift_tab` hop to the next/previous editable
  cell through `advance_edit`, staging this one, and `on_escape` discards and calls `refocus_grid`
  unconditionally. `on_blur` discards on a click-away; the field defers it a tick so a click inside
  it is not a blur, which is why it asks `gs.alive()` (a switched tab has disposed the grid by then)
  and that `edit_cell` still is this cell (a Tab/Enter hop has already repointed it, and the blur
  must not clobber that), and it takes the keyboard back only when `reclaim_keyboard` finds the
  pointer over the grid — Floem 0.2 gotchas has why the pointer decides. **Every cell is edited
  `multiline`, whatever its value** (`cell_field_multiline`; `no_wrap`, `enter_never_breaks`, one
  line tall either way): a database cell is the one field where a line break is data, and a
  single-line field strips breaks from its document and writes the stripped text back
  (`single_line_sync`). Deciding it from the seed — multiline only when the value held a `\n` or
  `\r` — covered only the first cell opened. `start_edit` seeds the shared `edit_buf` *before*
  `edit_cell` moves, so on a Tab/Shift+Tab/Enter hop the field being left is still mounted and its
  sync reads the next cell's value first; a single-line one joined its lines, the destination opened
  on the joined text, and the next hop or Commit staged a value nobody typed. Two lines pasted into a
  one-line value lost the break the same way (`cell_field_tests`). A bare field shows no scrollbar
  for being multiline — the `Handle` class draws the fading bar only for `multiline && !bare`, since
  one line tall it would sit over the text. A numeric cell right-aligns
  through `FieldCfg::inset_left` (*No `text-align` in Floem 0.2*, under the gotchas), and a date
  cell's calendar has its own blur guard (*Dates keep the text input*, below).
  **What the commit's landing clears is addressed by position and by key, and ✗ Discard is live
  while the write is in flight.** `commit_grid` captures a `CommittedStaging` — the staged cell
  keys, how many pending rows there were, the rows marked for deletion — and `drop_committed_staging`
  drains exactly those when the write lands, because the app skips the re-run when the committed tab
  is no longer active and then nothing else un-stages what was written (pressing Ctrl+Enter again
  inserted the row a second time; a re-issued `DELETE` matched 0 rows and rolled the batch back).
  **That skip is `model::settle_after_switch`**, which answers both halves of "the tab moved on" at
  once and is the write-back report's last piece of logic to leave `app_view` for core: whether to
  launch the re-fetch, and what to report. A `Spliced` for a grid that is no longer on screen is
  reported as `FullReran` rather than delivered — the rows are right, but claiming a splice would
  tell the reader their edits were reconciled into a view that never received them, which is the
  report claiming more than was delivered. A `Failed` is unchanged by a switch: the message is still
  true and the staged edits are still there to retry.
  Discard stays live deliberately — a write can sit on `innodb_lock_wait_timeout` for fifty seconds
  and taking the user's only way out of that is worse — and it empties all three kinds, after which
  *"the first `staged_new`"* names a different row: stage a row, commit, discard, stage another, and
  the landing drained the row the user was still typing, with no error and no trace. The
  key-addressed halves fail the same way, a re-edit of a committed cell carrying that cell's own key
  and a re-staged deletion of row 5 still being row 5, so the guard is **one question asked once**:
  `PanelView::staging_gen` is bumped wherever the trio is thrown away wholesale (Discard, and
  `clear_staged` on a panel load), the commit snapshots it, and a landing whose generation has moved
  clears nothing. It lives on `PanelView` rather than on `GridState` for the reason the trio does —
  its one consumer is an async landing that may arrive after the grid it started in is gone, and a
  signal created in the grid's scope would read "not mine" every time, which is the bug this exists
  to fix. The splice arm puts the server's rows in either way (they are the server's, and the grid
  should show them) and makes only the *staging* half conditional.
  `drop_committed_staging` takes the four signals rather than a `GridState`, which cannot be built
  in a test — exactly how this path came to have none.
- **A column whose legal values are written down doesn't edit as text.** Booleans, `ENUM`s, `SET`s
  and dates get their own control, in the grid *and* in the row panel, over
  `core::celledit` + `core::date` (the rules) and `ui::cell_editors` (the widgets) — both of which
  say why each shape is what it is. What the grid adds is the plumbing:
  - **`gs.editors`** holds one `CellEditor` per result column, resolved by `column_editors` from
    the column's **declared** type — its base column in the loaded schema, falling back to the wire
    type. It is a signal filled by its own effect, with **tracked** reads of `db_nodes` and each
    node's `schema`, so a database whose introspection lands after the grid was built upgrades a
    text field into a dropdown in place. (`edit_model`'s effect deliberately does not track those;
    the two answer different questions and editability is settled at build.)
  - **The column's control is narrowed to the value in hand** before it is used, by `fitting_editor`
    (row panel) and the same `celledit::fits` call in `data_cell`'s editing branch. This is the seam
    the whole feature's safety rests on: the classification is per *column*, and one cell of that
    column may hold something the control cannot represent.
  - **In a cell**: booleans, enums and sets all edit through one control
    (`cell_pick_editor` — the value and a chevron, with the shared popup menu over it), opened
    against the cell one tick after the face is built because a view has no `layout_rect` until it
    has been laid out. A cell has room for a value and a chevron and nothing else, and a menu is the
    only list that can be drawn over a grid at all. Choosing commits immediately through
    `keep_cell_edit` — Enter's contract, including the pending-row hop — so a `SET` toggles one
    member per opening. The editor closes on **any** dismissal of that menu, which is why it watches
    the `popup` flag rather than only its own actions: the root's click-away handler writes that
    channel directly. **The cell drops its own horizontal padding** while such a control is open
    (`cell_fills(&open_cell_shape(..))`, asked by both the content and the style so the two cannot
    disagree): the control carries its own surface, and a padded cell around it reads as a box
    inside a box. What an open cell *is* — a plain field, a picker in place of one, or a field with
    the calendar beside it — is the pure `cell_shape(editor, buf)`, three shapes rather than "a
    control or not" because the two controls want opposite things from the cell around them.
    **And the menu may not outlive the cell.** The popup channel is window-global and nothing but a
    pointer-down anywhere clears it — not a tab switch, not a re-run, not a scrolled-away row — while
    the entries are `Rc` closures over this cell's signals, so clicking one after the scope is gone is
    a `get_untracked` on a disposed signal, i.e. `try_get_untracked().unwrap()`: a panic that takes
    the window and every other tab's uncommitted edits with it. `cell_pick_editor` and
    `cell_editors::pick_field` therefore carry the `on_cleanup` → `cell_editors::close_picker` that
    `cell_calendar_editor` already had, and `close_picker` takes down only a menu standing at **its
    own** anchor. That anchor is *remembered*, not recomputed: `open_picker` returns the anchor it
    left standing and each control holds it in a plain `Cell`, because a view's `layout_rect` is not
    something to ask about while its scope is being disposed and a signal read inside `on_cleanup` is
    the very hazard the cleanup exists to prevent. `keep_cell_edit` is the belt to that brace, and
    opens with the `gs.alive()` guard `drop_cell_edit` already had.
  - **In the row panel**: `typed_editor` puts the control inside the same NULL toggle a text field
    gets (`nullable_field`, extracted from `scalar_editor` for exactly this). Only the `SET`'s
    wrapping chips can outgrow a line, so only that row grows.
    **Every one of those controls owes the keyboard what the field it replaced gave it** — the
    panel's `autofocus` and its panel-closing Escape — and two of them ignored both, which is a
    column that cannot be set without a mouse: nothing took the keyboard when the panel opened on
    an `ENUM`, and Tab walked past the control as though it were a label. They are
    `keyboard_navigable` now, which is also their activation: floem fires `Click` on the focused
    view for Enter and Space, so the pointer's handler is the keyboard's handler, and a `SET`'s
    chips are a stop each because a row of independent toggles is what a checkbox group is. The
    contract is gated by a signature scan (`row_panel_focus_gate`) rather than trusted, since what
    went wrong was two parameters that were never passed.
    **The layer above it has its own gate, because a wrapper can withhold the keyboard before any
    control exists.** `nullable_field` and `json_field` each have a branch that builds no control at
    all — a nullable column that is NULL in this row renders the `<null>` sentinel and a **Set
    value** button — and both took `autofocus` only for the other branch, so a panel opened on a row
    whose first editable column was NULL took no keyboard: the arrows went on moving the grid's
    selection under an open panel, and reaching the first field cost a Tab walk or a click. All
    three wrappers (`field_mini_btn` included) now take it through, and that button is
    `widgets::key_pressable` + `cell_editors::focus_on_mount` — the first because Tab walking past a
    button as though it were a label is the other half of the same complaint, the second because
    focusable-on-mount and reachable-by-Tab are different properties. `grid::row_panel_null_gate`
    pins both, a source scan in the family with `popup_anchor_gate` and `menu_trigger_gate` and for
    the same reason: what went wrong was not a calculation but a parameter that never arrived.
    `key_pressable` is `in_ring_button`'s no-`FocusRing` sibling — the row panel and the activity
    panel join floem's own traversal, so the registration is `keyboard_navigable` in place of
    `in_focus_ring` — and Enter and Space press it while **every other key carries on**, so Tab still
    walks past and the panel's Escape still reaches the panel. **The registration is not the only
    difference between the two helpers: this one binds the pointer itself.** `in_ring_button` splits
    the pair, its callers wrapping the *view* they hand it in their own `on_click_stop`;
    `key_pressable` builds the container it wraps, so a caller's click listener would sit inside the
    focus ring rather than on it, and leaving that half to the caller shipped **both** of its callers
    dead to a mouse click — this button and the activity panel's lock-wait Kill, each of which
    *replaced* an existing `on_click_stop` with the helper to gain the keyboard and lost the mouse in
    the same line. It wires `.on_click_stop` on that container now, sharing one `Rc`'d action with
    the KeyDown arm, and `widgets::key_pressable_gate` holds it to both listeners and checks no
    caller binds the click a second time — one press running the action twice is invisible for an
    idempotent *Set NULL* and is not for a Kill. `focus_on_mount` is `pub(crate)` for the same
    sharing.
  - **Dates keep the text input** in both places, with the calendar beside it: typing a date is
    often faster, a `TIMESTAMP`'s time of day has no calendar to come from, and a value no picker
    can represent still has to be editable (`0000-00-00` gets a plain field and no panel at all).
    In a cell the panel is dropped from the cell's own rect, one tick late for `cell_pick_editor`'s
    reason, and `cell_calendar_editor` hands it the editor's **lifetime** rather than sharing it:
    - **The keyboard cannot decide when the editor closes while the panel is up.** Floem takes the
      window focus on *every* pointer-down and hands it back only to a focusable view under the
      cursor — a day, a month arrow and the Now button are none — so the first click inside the
      calendar reached the field's blur as "the user left", closed the editor, and the pick it was
      about to make landed on a cell that was no longer being edited. `data_cell`'s `on_blur`
      therefore stands down for a press the panel reports having taken, and the panel
      closes the editor instead: a chosen day stages through `keep_cell_edit` (the in-cell picker's
      commit, a cell having no Save button in reach) and every other way the panel goes away closes
      the editor without staging, which is the effect on the channel.
    - **The press is the question, not the panel.** `cell_editors::take_calendar_press` is a
      one-shot flag set by the panel's own pointer-down swallow and *taken* by the first asker,
      because floem clears the focus, dispatches the press, and only then emits the `FocusLost` it
      caused — so a flag set during dispatch is readable by the handler that follows it. Standing
      down for the whole time a panel was merely *open* was the first spelling, and it ate Escape:
      the field was floem's `text_input` then, which answers Escape by dropping the window focus
      and reporting the key handled, so that key reached this blur and nothing else in the app, and
      swallowing it closed neither the editor nor the panel and left the grid with no keyboard.
      (Escape is the field's own `on_escape` now and never arrives here; the press-not-panel rule
      stands for the real blur — a click elsewhere — which must still close the editor while the
      panel is up.) The field also drops a standing flag when it regains the caret, the one moment
      it is certainly stale — through `FieldCfg::on_focus`, set on the field where it is built. A
      `FocusGained` listener on the view around it, which is where this used to be, never fires:
      the focus lands on `edit_field`'s inner editor, and no listener outside it hears that.
    - **Standing down is only half of it: the guard hands the caret back.** Nothing else will —
      floem returns the focus only to a focusable view under the cursor, and a day is not one — so
      a field that merely survived the press was mounted and deaf, with a `DATETIME`'s time of day
      untypable and Enter/Tab going to the window root. The hand-back is a pulse of the field's
      `FieldCfg::caret_end` (a counter bumped with `try_update`), not a `request_focus` on an id:
      the field refocuses itself with the caret at the end of the value, which is where
      `text_input` used to put it on regaining focus.
    - The field keeps working throughout — it still holds the value, Enter still stages it, Tab
      still hops — and its teardown clears the channel, because `edit_buf` belongs to the *next*
      cell to be edited as much as to this one.
- **Range selection is back, and it feeds the aggregates bar.** The state (`active`/`anchor`), the
  rect (`bounds`), the paint and `copy_selection` were always multi-cell aware; only the *input*
  had been gated off ("the grid has no multi-cell actions"). Shift+click, Shift+arrow and
  drag-select are live again — drag needs no pointer capture, just `gs.selecting` set on a cell's
  `PointerDown`, extended by each cell's `PointerEnter`, and cleared by the **whole grid's**
  `PointerUp` (floem dispatches a pointer event to the first hit child in reverse paint order and
  stops, so a release over the frozen pane, the header or past the last row never reached the data
  body's own copy — and the flag stayed armed with no button down, the selection following the bare
  cursor) **and by the double-click handler**, since floem's `DoubleClick` swallows the second
  `PointerUp`. `Del` drives the whole range to *one*
  state rather than flipping each row (on a mixed selection a per-row toggle both marks and
  unmarks, which reads as the key doing nothing) — that vote is `delete_vote`, and it is applied in
  **one** `del_rows.update` and one `dirty.update`: `toggle_delete` per row was two notifications
  each, so Ctrl+A then Del at the 200k row limit fired 400,000 of them and locked the window, on
  the two-keystroke gesture the feature exists to enable. The write is `GridState::mark_deleted`
  (over `mark_rows`), shared with the gutter menu's *Delete N rows* — which followed this rule late,
  and whose entry above carries the measurements — and it asks the join guard.
  **Which column the arithmetic is about is the *anchor's*** — the one the selection started on, so
  dragging from `price` across to `name` still reports `price`. It reads `gs.anchor` rather than
  `bounds()`, which is a normalised rect and has forgotten which corner you began at. A selection
  covering every *drawn* column is a row selection (gutter click, Ctrl+A, the Ctrl+G jump) whose
  anchor column is the first — usually an id, whose sum means nothing — so those get counts only; a
  single-column result is exempt, since there covering every column is covering the one you meant.
  *Drawn* because the row gestures span `ColLayout::edges`: asked against the raw column count,
  Ctrl+A with column 0 hidden summed column 1, so `selection_kind` takes `ColLayout::shown` and the
  effect tracks `hidden` (`a_row_gesture_over_hidden_edges_reads_as_a_row_selection`).
  Those three rules are `selection_kind`, extracted and tested — they were inline in the effect
  while the arithmetic they gate had seventeen tests, so flipping `ncols > 1` to `>= 1` broke the
  exemption with nothing failing. The effect tracks `rs`, `order`, `dirty` and `new_rows` as well
  as the selection, and reads each cell **as the grid draws it**: tracking only `active`/`anchor`
  left the previous total standing under a sort or a commit splice, and a staged green edit was
  never in it at all. A staged **blob** is a value there and not a NULL: `CellEdit::as_text` answers
  `None` for `Bytes`, which is what every *text* caller wants and what this one read as absence, so
  loading a file into a selected cell turned `3 rows` into `3 rows · 1 null` while the stored blob it
  replaced counted as a value — the grid holds `<n bytes>` for that one, and that is a text. A
  staged `DEFAULT` is neither, counted as a pending row's unset cell is: the effect reads
  `edit::GridCells::summary_cell` and folds with `aggregate::aggregate_counted`, whose
  `Counted::Unset` is a row and never a null. Through `summary_text` both were `None` and printed
  as nulls, while this sentence already said otherwise.
  `grid_selection_bar` renders at panel level (like the find bar, so it can sit at the panel's
  edge) while `grid_view` computes it, and it lifts itself above `grid_error_bar` when that one is
  up: they coincide exactly when a bulk delete fails, which is when both have something to say.
  **Each figure is clicked to copy it.** `GridCtx::sel_summary` carries a `SelReadout` — the
  column's name and `Aggregates::figures`, each shown as the summary shows it (`Sum 1,234.50`) and
  copied as the bare value (`1234.50`: no label, no grouping commas, which a spreadsheet or a
  `WHERE` would read as text; `each_figure_copies_as_its_bare_value`) — rather than the one
  flattened line, and `Aggregates::summary` is now those figures joined, so the summary's tests
  guard both. **What the chip shows is worded from the figure's `what`, not its `shown`**:
  `FigureWhat` is `Rows { n, more }`, `Nulls { n, more }`, `Distinct`, `Length { lo, hi }` or
  `Stat { stat, value }` — the value **ungrouped**, so `grid::figure_text` groups it in the
  interface's marks through `aggregate::grouped` and words `Stat`'s `Sum`/`Avg`/`Median`/`Min`/`Max`
  from `grid-figure-*` (`a_stat_carries_its_value_ungrouped_for_the_interface_to_mark`,
  `english_reads_every_selection_figure_as_core_does`); `shown` stays core's English. A clicked figure reads *Copied* for `FIGURE_COPIED_FOR`, behind a generation so a
  second click is not cut short by the first one's reset, and **it is withdrawn when the readout
  is rebuilt**: keyed by position and outliving the chip, it landed on whatever figure the next
  selection put in that place, a value nobody copied. The click then hands the keyboard back
  (`widgets::hand_keyboard_back`, which reaches the grid as the keyboard home): floem clears the
  focus on every pointer-down and returns it only to a focusable view, so the arrows and Ctrl+C
  reached nothing after a copy. The bar was `.pointer_events(|| false)`
  while it was one line of text — as the last child of the results stack it swallowed every click
  on the cells it covered — and **the flag had to go rather than be undone per figure**: the
  opt-out is not inheritable (above), so a flagged container hides its children from the hit walk.
  What it takes now is the chip's own shrink-wrapped box, and nothing while hidden (the absolute
  placement applies only while there is a readout); `grid_error_bar` keeps its events as before.
  **A wheel over the chip scrolls the grid**: floem hands a pointer event to the first hit child
  only, and the grid is the chip's sibling, so `grid_view` publishes its `forward_wheel` in
  `GridCtx::sel_wheel` with each body it builds and the chip calls it while it has a readout.
  **Every panel-level bar is cleared when the result stops being `Loaded`**, in one
  effect in `results_section`: their only writer lives inside `grid_view`, so a failed re-run left
  the previous total and a live-looking Go-to-row popup pinned to a panel saying "Query failed."
  That means the **shown** result (`shown_panel_loaded`), not "any result in the tab": only one grid
  is mounted, so switching from a loaded Result 1 to a failed Result 2 was the same bug one level
  along.
- **A failed statement reports in the editor's error bar, and only there.** The pane keeps a dim
  "Statement failed." — a server error is one long line, and as the pane it rendered unwrapped
  across the middle of the window and out over the schema sidebar. The message goes under the SQL
  that produced it, beside the **Explain** and **AI fix** that act on it (`intel::error_fix_range`
  scopes the fix to the failing statement, so this works for one statement of a script as well as
  for a whole run) — each offered only as far as the error's `ErrorSource` earns it, which is why
  `ShownResult::bar_message` hands the bar the whole `ModalError` rather than its text
  (`only_a_failed_result_reaches_the_error_bar` checks the source survives). It used to be split: a single run's went to the editor bar and a batch
  statement's to the panel bar (`batch_err`), because `run_all` cleared the tab's `results` and left
  a batch with no editor bar to fall back on. Once **every** result became a panel the two were the
  same value, and the pair drew the same error twice — so `batch_err` is gone and `grid_error_bar`
  now reports only on what the *grid* did: a commit, a filter re-run (an export's state moved to
  the export modal — see the export bullet below). None of those is a
  statement in the buffer, which is why `BarState::Error` no longer carries a `fixable` flag — it
  was `false` on every error once the batch arm left — and its **View** shows when
  `ModalError::worth_viewing` says so and hands the modal the `ModalError` exactly as it was set.
  **The source is chosen where each error is produced, not by the bar**, because `commit_err` and
  `view_err` (`RwSignal<Option<ModalError>>` on `GridState`, `GridCtx`, `BarSignals` and
  `Tab::view_err`) carry both speakers. `Server`: `commit_writes`' error — the one-row net's
  verdict included, which describes the server's answer — and a view re-run's
  `QueryState::Failed` once `ModalError::about_another_statement` has turned a `Statement` into
  one: the re-run's SQL is Schemaic's, and a fix from the modal would land in the editor's buffer
  (a re-run's own `App` note, a timeout say, stays `App`). `App`: a `commit_writes` that ended in `DbError::Cancelled` (the user
  stopped it, and "query cancelled" has nothing in it to explain) or in "Commit cancelled — …",
  this crate's account of a Stop whose rollback did not undo everything — a `DbError::Refused`
  once, a `DbError::Write` of `WriteCause::CommitCancelled` now — which the
  arm's hand-written "`Cancelled` is `App`, everything else `Server`" had offered *Explain* on; the
  arm now asks the app's `modal_error_shown`, `DbError::modal_about_another_statement` shown in
  the interface's language — the bar draws `ModalError::displayed()`, while the `text` *Explain*
  sends stays English — instead; a paste that landed nothing,
  the row panel's and the JSON editor's
  refusals, AI fill/seed failures, the filter row's three refusals (a `BadCondition` is
  `sqlparser`'s reading of the typed condition, not the database's), the `db_for`/`session_for`
  refusals in the commit path and in the view re-run's `fail` closure, and the "Not connected —
  the commit was not attempted" gate. The bar used to hard-code `ModalError::server` on all of
  them, so *Explain* was offered on Schemaic's own notices, while its *View* keyed on
  `hides_detail` alone kept a one-line commit failure — the error *Explain* is for — out of the
  modal entirely.
  The editor bar keys on a `Memo<Option<ModalError>>` of the message rather than on the `QueryState`
  itself: the shown result is derived from the panel list, so every write to it — each statement of
  a batch landing, a pin, a filter re-run restating its panel — reaches that container, and
  `QueryState` has no `PartialEq` to dedup on. Keyed on the state it rebuilt the bar on all of them,
  including on every keystroke, since typing clears a stale error through the same signal.
- **That bar has three states on two surfaces, and the surface is the message.** The red fill means
  an error; the ordinary chrome carries the wait note (a write taking long enough to explain, with
  its one-click `Rollback`) and a plain **note** — something worth saying about an operation that
  *worked*. There was a fourth, a running export, and it has gone with the whole of the bar's export
  state — see the export bullet below. The note exists because there was no non-red channel: a
  partial paste used `commit_err`, so an ordinary success was drawn in the colour that means a
  write-back failed. The four signals travel as one `BarSignals`, whose `any_up` is asked by the
  bar's own style **and** by the selection summary that lifts itself above it — two hand-written
  copies of the same `is_some` chain before, which a new surface would have had to be added to
  twice, and a bar that is up while the summary thinks it isn't is the two of them drawn on top of
  each other. `grid_error_bar` binds it **exhaustively, with no `..`**, which is what the struct is
  for: a rest pattern re-opens the exact failure — a field added here and bound nowhere, so a bar the
  rest of the app is filling never appears. The `..` became vestigial when the export state left this
  bar, and a vestigial rest pattern is indistinguishable from a deliberate one.
  **What travels in `BarSignals` is state, never an action**: `rollback_tx` is an
  `Rc<dyn Fn(usize)>` parameter of `grid_error_bar` rather than a field, because the struct is
  `#[derive(Clone, Copy)]` and an `Rc` in it would cost every reader of `any_up` a clone.
  `export_cancel` was the second such parameter until the export's Stop moved to the modal, which
  reaches it on `TabsActions` directly. `results_section` reads the action off `GridCtx`, so
  `GridState` keeps no copy of it. On the writing side, `GridState::clear_bar` is the one way the bar comes down: seven
  identical copies of `if commit_err.is_some() { … }` were what a second signal would otherwise have
  had to be added to seven times. **`discard_edits` takes the whole bar down, not just its error
  surface** — Discard means "none of that is true any more", and the *note* surface is the one that
  can hold a sentence about the edits just thrown away, so `Pasted 5 cells, skipping 1 in read-only
  columns.` stood over five cells that no longer existed until some other path happened to clear it.
  That site read `clear_if_any(gs.commit_err)` rather than the `is_some()` shape, which is why the
  sweep that found the other two copies of this missed it.
- **A result says which database it came from, and the answer lives on the result.** The grid's
  stats line leads with `ResultSet::database` (`world · 100 rows · 15 cols · 1 ms`), stamped by the
  loader that knows the scope — `Db::fetch_query`, `Session::fetch_query` (its *pinned* database,
  which need not be the tab's current one) and `Db::run_batch`, which wraps its `on_result` sink
  once so neither engine's path can forget. **Not read from the tab**: a tab's selection moves, so a
  result outlives the database it ran under the moment someone changes the selector, and a label
  sourced from the tab would be wrong in exactly the case it exists to catch (the other half of that
  bug — a result landing in a rebound tab — was fixed by cancelling the run). On the result it is a
  snapshot by construction and survives a commit splice, which mutates the columns in place;
  a test pins that, because the label vanishing on first edit would strand it exactly when the user
  is writing to whatever it names. It is also **per result**, which a status-bar or footer line
  could not be: Run Everything renders a grid and a toolbar per statement. `None` (a connection with
  no default database) prints nothing rather than inventing a name, and the field names the
  statement's *scope*, not the origin of every row — a qualified `world.country` read while scoped
  to `sakila` still reports `sakila`. **The scope follows a `USE`**, in both `run_batch` and
  `Session::fetch_query`: it was computed once, on a method whose own doc advertises that a `USE`
  carries across statements, so `USE sakila; SELECT * FROM actor;` from a `world` tab really ran
  statement 2 in `sakila` and labelled it `world` — the label lying in exactly the case it exists to
  catch. `sql::use_target` reads it and is deliberately conservative: a `USE` it can't read plainly
  drops the label to `None`, which prints nothing, rather than carrying a name now certainly wrong.
- **A capped result says what it capped, when it can say it honestly**: the stats line reads
  `1,000 of ~4.2m rows` rather than `1,000 rows (capped)`. Three things have to line up, and
  `grid_view`'s `row_total` memo is where they do. The read must be capped
  (`ResultSet::truncated`); `grid_query` must be empty, because a spliced header filter re-runs a
  statement that is *not* `base_sql`; and `base_sql` must return the table entire
  (`intel::full_table_source`), or the table's estimate is a total this query never had. The figure
  then comes from `ConnNode::stats` via `stats::catalogue_key` + `SchemaStats::find`, requested
  through `SchemaActions::db_stats` and read reactively so it appears when the fetch lands — which is
  why the line is a `label` and not a `text`. **The ask tracks `SchemaUi::stats_gen`**, like the
  tree's size column: a schema Refresh resets every slot to `Idle`, the memo that prints the figure is
  live on that slot and dropped it immediately, and an ask that ran only at grid-build time meant
  `of ~2.84m` vanished from an unchanged on-screen result and never came back — unless the *opt-in*
  size column happened to be on and that database expanded, which is the tree's refetch and not this
  one. A repeated ask is free: the slot is the guard. The wording of the segment — including
  whether `(capped)` is still worth saying once the comparison is there, and which figure the noun
  agrees with — belongs to `stats::rows_read`, not to the view; the view supplies
  `truncated` and the total and words the `RowsRead` that comes back (`rows_read_text`). The cap itself is unchanged and is still a
  **client-side stream cutoff**, not a `LIMIT`; this only says how much of the table went past it.
- **A table tab pages; the pager is the `LIMIT`/`OFFSET` the cap is not.** `grid_toolbar`'s
  `pager` sits after the read-more offer — `· ‹ Rows 101–200 ›`, the words from
  `filter::page_caption`, counted from the base's own `filter::page_offset` — and is drawn when `filter::page_size` answers for `base_sql` **and**
  `GridState::rerunnable` holds. So it is not a table tab's alone: any single-table `SELECT` with a
  constant limit gets one, a table tab's `table_query` simply always qualifies. The second term is
  the frozen rule and the write guard at once, through the funnel everything that re-runs already
  asks — a kept result draws no pager, since turning a page re-runs into the very panel that was
  pinned. A chevron's click hands `apply_grid_query` the change `|q| q.page = to`, so a page is
  minted through `RerunRequest` like a filter, and composed after the filter and sort
  (`filter::compose`, whose entry has the SQL each engine gets). **Every other change to
  `grid_query` sets `page` back to 0** — filter-by-value, `cycle_server_sort`, the filter bar's
  apply and its clear ✕ — so a new filter or order starts again at its first page. **None of them
  writes `grid_query` itself**: each passes its change to `apply_grid_query` as a closure, and that
  action is the one writer (`the_grid_query_is_written_only_by_the_guarded_action`, a source gate
  in `header_sort_tests`). The callers used to write the query and then call the re-run, so a
  refusal inside it would still have left the bar's ✕, a header's chevron or the page caption
  describing a query that never ran — and a fifth caller written later could skip the guard
  outright. The click is refused while `Tab::view_busy` holds,
  and both chevrons drop to `text_faint` then, as they do when there is no page that way —
  *Previous* on page 0, *Next* after a short page. **Neither turns over staged work**
  (`filter::page_turns`, `a_page_does_not_turn_over_staged_changes`): a page is a re-run, and its
  landing clears the grid's staged edits, new rows and deletions with the rows they sat on, so
  turning used to throw them away without a word. While any are staged both chevrons are dimmed
  and their tooltip reads *Commit or discard your changes to change page*; whether anything is
  staged is in the pager's rebuild key as a bool. **The pager was the only one guarded, and every
  other re-read had the same hole**: the filter bar's apply and ✕, a header sort and *Filter by* /
  *Exclude* all re-ran regardless, and the landing (`Tab::bump_panel_load` →
  `PanelView::clear_staged`) dropped the staged trio without a word. So the guard is now on the
  action as well: `apply_grid_query` asks `filter::requery(current, change, staged)` **before**
  making the change, and over staged work (`Err(HoldsStagedWork)`) leaves `grid_query` untouched,
  puts `i18n::staged_requery_note` — `filter::STAGED_REQUERY_MSG`'s words, *Commit or discard your changes first — re-reading the rows would
  throw them away* — in the bottom error bar (`view_err`), and returns `false`
  (`a_requery_does_not_run_over_staged_changes`). **The three refusals after it do the same**:
  `grid_query` is written only once the re-run is approved and on its way, so the write guard, an
  un-rewritable base and a bad condition also leave it alone and return `false`
  (`the_grid_query_is_written_after_the_rerun_is_approved`, a second source gate). It used to be
  written first and those three returned `true`, so the ✕ emptied the bar over a refused re-run
  and the grid showed filtered rows under an empty filter bar. The ✕ empties the field only on
  `true`, so a refused clear does not show an empty bar over a filter that is still applied; a
  refused apply leaves the typed text in the field to fix, or for after the commit. Those controls refuse rather than
  dim, unlike the chevrons, because the filter field is where the user types the next filter and
  a header click is also the client-side sort's control on every other result.
  `GridState::has_staged` is the one spelling of "is
  anything staged" for the pager's key, the action and the capped notice's read-more; it reads
  tracked, because the pager's key has to see the first staged edit — **through a memo**
  (`staged_key`), since `dyn_container` does not dedup and a bare read rebuilt the pager, and
  re-parsed the base four times, on every staged cell (`the_pager_key_reads_staging_through_a_memo`)
  — and the other two ask from click handlers where there is nothing to subscribe. **Those guards
  ask at launch, and two holes were past them.** A view re-run's landing is decided by
  `filter::view_landing(loaded, staged_now, pending, shown)`: `grid_query` is written at
  dispatch, before the server answers, so a sort the server refused (SQL Server's Msg 305 on
  `xml`, PostgreSQL's on `json`) stood as the tab's query — the chevron over unsorted rows, every
  page and later re-read replaying it, a commit's re-read failing on it — and a landing that
  replaced the rows cleared edits staged *while it ran*, unasked. Now a refusal puts back
  `Tab::grid_query_shown`, the query the shown rows came from, and a landing onto staged edits
  keeps the rows, the edits and that query, saying `i18n::staged_landing_note`
  (`filter::STAGED_LANDING_MSG`'s words)
  (`a_view_landing_keeps_the_shown_query_unless_it_shows_the_rows`). **And a run from the editor
  asks**: Run and Run All replaced the result holding staged edits, and they went with it, so the
  run guard folds `Tab::staged_on_replaced_panels` into its verdict
  (`filter::with_staged_question`, which folds the question in as `GuardReason::Staged { after,
  changes }`, `after` being the reason the run was already asked about, and `staged_run_note`, the
  English its `message` says) — an allowed run asks first, *Run anyway*
  discarding them (`a_run_over_staged_edits_names_what_it_discards`). The chevrons carry *Previous page*/*Next page* tooltips
  otherwise, and they are plain clickable containers rather than `in_strip_button`s, so F6's ring does not
  reach them.
- **The Download menu names the scope, and only when there is a choice to make.** `export_menu`
  normally opens the flat six formats — all of them, unlike the **Copy** menu beside it, which
  filters on `export::ExportFormat::is_text` and so has no Excel entry: a file can hold bytes and a
  clipboard render cannot (the empty-string trap is in `core::export`'s entry). When the result is
  `truncated` **and** `GridState::current_statement` yields something to re-run, it puts a scope
  step in front of them —
  `Fetched rows (N)` and `All rows (~N)`, both through `text::human_count`, the `~` for the same
  reason the stats line carries one (the figure is the catalogue's estimate, and a menu promising an
  exact count the export then missed would be worse than one that never claimed it). **The step
  appears only when the two scopes differ**: an uncapped result *is* every row, so offering to fetch
  them again would be a choice between a thing and itself, and a result with no statement to re-run
  has only the honest scope left. **"Something to re-run" already includes "may be re-run"**: the
  write guard is *inside* `filter::rerun_statement`, which `current_statement` wraps, not a second
  term ANDed on here — the term this menu used to carry could be deleted with the whole suite green
  (see the write-guard invariant). It is a refusal by omission: the scope is simply absent, so it
  cannot be asked for. `All rows` is
  `ExportScope::AllRows`, which carries the **statement** rather than the table — a filtered or
  sorted grid exports all of what it is showing, and a table tab's statement is `SELECT * FROM t`
  anyway — plus the *result's* `database` and `GridState::conn_at_load`, so **both halves of "where
  did this result come from" answer as of the same moment**. The `conn_id` used to come off the live
  tab while the `database` deliberately came off the result, and a tab rebound to another connection
  keeps its loaded result on screen: the two then disagreed and the export could re-run the
  statement against a different server. `conn_at_load` is snapshotted in `GridState::new` from the
  same `gctx.conn_id.get_untracked()` the format seeding already reads. The `All rows` export is
  deliberately a **second read** of the server rather than a continuation of the first: the rows on
  screen may be minutes old, and stitching a stale page onto fresh ones would be neither.
  **The formatter's *save* reads `conn_at_load` too now**, through `format::rule_key`, which is the
  same disagreement one surface over: the seed keyed on the connection the rows came from while
  `set_format` keyed on the tab's live one, so a rule set after a rebind was written against a server
  the rows never came from — absent where the user set it, and applied to a real, different table on
  the other connection (see `core::format`). The
  statement is snapshotted before the save dialog opens for the same reason the rows are — the
  dialog is modal and slow, and a filter typed while it stood open must not change what the export
  was asked for. **Neither menu opens on an empty result**, and both used to: they offered their
  format list on a 0-row result and cheerfully wrote a header-only CSV, a bare `[]` or an empty
  Markdown table — a file that describes the query rather than answering it, and a clipboard paste
  that looks like a failure. The gate is `grid::results_offer_export(rows)`, **rows and not
  columns**, which is the whole of the distinction: a 0-row result still carries its column list, so
  every format can render *something* from it, and that something is exactly what is worth refusing.
  The formats are not wrong to produce it — an export of an empty result should be an empty result,
  not an error — the menus are wrong to offer it
  (`grid::tests::an_empty_result_offers_the_toolbar_menus_nothing`). Deliberately **not** the schema
  tree's `Export ▸`: that entry names a *table*, whose row count nothing on screen has read, so a
  greyed entry there would be a question the user cannot answer by looking, where here the stats
  line one row above already says `0 rows`. **The gate is on the action, not on the face** — the
  pointer click and the ring's Enter/Space both funnel through the `open_copy` / `open_save`
  `Rc<dyn Fn()>` closures, so the early return lives inside those, and dimming the icon alone would
  have left the keyboard path opening a menu drawn as unavailable (*Buttons are in the ring too*).
  And `has_rows` is a **live closure, not a build-time `bool`**: the toolbar outlives a re-run that
  replaces the rows under it, so a flag read while it was built would gate the menus on the previous
  result. It reads `gs.order` rather than the result set, because `order` is what an export actually
  writes; the two agree today and the one that decides the file is the honest thing to ask.
  **The chart icon takes the same gate and the same shape** — `icons::CHART_COLUMN` (Lucide
  `chart-column`) at `TB_CHART` (65), between the AI menu and copy, an `in_strip_button` registered
  unconditionally with its `open_chart` built by `gated_action(has_rows, …)` and its face dimmed by
  `menu_icon_color_gated`. It raises the *Chart* modal (`chart_view.rs`) rather than a menu, by
  setting `GridState::chart` — `GridCtx::chart`, which `lib.rs` fills with `ui.overlay.chart` — to
  the result on screen, so it has no toggle. **It asks only whether there are rows, not whether any
  column is numeric**: that is the modal's to say (*"No column of numbers to chart in this
  result."*), rather than a scan of every cell each time the strip is built.
  **Every way the file will differ from the grid is declared at the point of choice, not discovered
  in the file**, and the label is `export::all_rows_label(size, sorted, manual_tx, staged)` — one
  tested
  function in place of the `match` the menu used to hold, which had none. Three differences, none of
  them visible in the result. A **client-side sort**: `Fetched` honours `gs.order`, while `AllRows`
  streams the *server's* order, because a column-header sort is a permutation of the rows in hand
  (`compute_order`) that no re-run reproduces — the menu had presented the two as one export at two
  sizes. And a **manual-transaction tab**: `AllRows` is a second read on a fresh connection
  (`Db::stream_query_enforced`, deliberately outside the tab's pinned session), so a `TxMode::Manual` tab's
  uncommitted rows are on screen and absent from the file, and rows it deleted are in the file and
  gone from the screen. And **staged edits**: this scope asks the server again, and the server has
  not been told, so it reads `without your staged edits` — a disclosure that became load-bearing at
  precisely the moment the *fetched* scope stopped diverging, since that file now goes through
  `exported_rows` — `insert_blocks_all` for SQL — and carries the green cells and the pending rows
  exactly as Ctrl+C does. Before
  that, both scopes were wrong the same way and "an export is of what was fetched" covered it. So
  the label reads `All rows (~16k, server order)`,
  `All rows (~16k, committed rows only)`, `All rows (server order)` where there is no estimate, or
  plain `All rows` when none applies. `size` is pre-rendered by the caller, since the estimate and
  its `~` belong to the stats line's vocabulary rather than to this decision; `sorted` comes in as a
  parameter of `export_menu` rather than off `GridState`, because the sort does not
  live there — it is `grid_view`'s, threaded to the header and the toolbar — `manual_tx` reaches
  it through the `tx_mode` signal `GridCtx`/`GridState` gained for exactly this, and `staged` is the
  menu asking `gs.dirty` and `gs.new_rows` whether either holds anything. **The estimate
  travels with the choice**: `export_menu` hands the same figure to `save_export`, which puts it on
  `ExportTarget::total`, so the number the user chose from and the number they then watch tick up are
  one read of the catalogue rather than two taken minutes apart.
- **The grid export has no bottom-bar state at all: the modal is its single affordance, from launch
  to dismissal.** The bar used to carry both halves — `BarState::Exporting` drawing `Exporting…
  Cancel` (`export_bar`) while the write ran, then the outcome sentence in the same strip, so one
  operation occupied one place from beginning to end instead of announcing itself in one surface and
  reporting in another. That was the right answer while the bar was the only surface, and the
  progress modal ended it: with a modal covering the window for every grid export, both scopes, the
  strip was a second place saying the same thing to nobody, behind a backdrop. Whether the running
  half should go was posed here as an open question; **it has gone, at the user's direction, and the
  outcome went with it.** Deleted with it: `grid::ExportRun`, `grid::export_finished`,
  `BarSignals::exporting` / `tab_id` / `exporting_here()`, `GridState::exporting` / `tab_id`,
  `GridCtx::exporting` / `tab_id` / `export_cancel`, and the window-scoped `exporting` signal in
  `lib.rs`; `BarSignals::any_up` lost its `exporting_here()` term, and `grid_error_bar` its
  `export_cancel` parameter — the modal reaches `ui.tab_actions.export_cancel` directly. **The
  fourth arm is worth a line on its way out**: `Exporting` was the only state standing for an
  operation *in progress* rather than for its result, and so the only one carrying a control that
  stopped something. What is left is uniformly the tail of something that has already happened —
  which is why `save_export` no longer takes the bar's stale messages down before it starts, there
  being no export state left for a leftover error to outrank, and why
  `the_bar_is_down_when_nothing_has_happened` pins that a bar holding none of the three is down:
  `any_up` also decides whether the selection summary lifts itself, so a bar that thought it was up
  for the length of a write would dim that summary throughout an operation it no longer reports.
  `GridState::clear_bar` keeps its contract unchanged — the commit pair, with the filter/sort error
  said separately under its own `is_some` guard, the way `dismiss_overlays` pairs the two — and is
  still deliberately not widened to swallow `view_err`, because it runs on the keystroke paths where
  the guard exists to keep a bar that is already down from invalidating its container.
  **Of the two questions the deleted `ExportRun { id, tab }` answered, one went and one survived.**
  `tab` was why the flag could outlive a tab switch, and it existed to keep `Exporting… Cancel` off a
  tab that had nothing to do with the export; with no bar state there was no reader left for it.
  `id` survives as `grid::next_export_id() -> u64`, monotonic per process and a counter rather than a
  token clone because comparison is all that is wanted; the question it answers did not go away:
  **which run owns the modal.** The lifetime lesson that moved the flag to the window survives
  too, one surface over — the export's cancellation token is app-global (`main::export_token`), so
  state with a shorter life than the token is a Stop that can vanish while the thing it stops runs
  on. The modal's signals are window-scoped where the bar's were built per active-tab render, which
  is a real gain rather than an accident of where they live: an export outlives the result that
  launched it, so a report arriving after a re-run used to land on a disposed signal and be lost
  entirely. The states that remain are unaffected by any of this. A stalled write's note
  (`commit_wait`) is **live, not stale** — the Rollback it offers is the most urgent thing on that
  strip. A statement's own failure means **no grid is mounted** to export from, so it never had to
  share the strip with one — and it is not this bar's message any more anyway: `batch_err` is gone,
  and a statement failure
  goes to the editor's error bar, under the SQL that produced it and beside the Explain and AI fix
  that act on it. (Which is why a failure now *un-collapses* the editor: `Collapse the editor` is a
  button in the RESULTS pane and it sets that pane to height 0, so the message, **View**, **AI
  fix** and **Explain** were all unreachable for exactly the run that needed them. Revealing the
  bar in place is not available — a child overflowing out of a zero-height parent is painted and
  never hit-tested.)
  `export_cancel` is a `TabsActions` action; the app owns the
  token, as it does for query runs and imports. The report
  is an `ExportOutcome` — `Done(export::ExportTally)`, `Cancelled`, `Failed { message, partial }` —
  three outcomes and not a
  `Result` because "the user stopped it" is neither: a red bar for a deliberate Cancel, or a
  cheerful row count for a file that stops halfway, are both lies. It mirrors `ImportOutcome`.
  **`Done` carries a tally and not a row count**, because two kinds of loss are invisible in the file
  itself: `withheld` is the binary columns whose `<n bytes>` placeholder was dropped rather than
  written as data (empty for Markdown and HTML, which keep it deliberately) and `blanked` is
  `ResultSet::capped_columns`, the cells past a column's 512 MiB text arena, which read back as the
  empty string. Nothing had ever read that flag, so a streamed chunk that overran the arena wrote a
  file with holes in it and reported a full row count. The success sentence is
  `export::export_said(&tally, &name, true)`, worded by `i18n::export_note` — `name` being
  `export-dest-file` for a path with no file name — and it goes into the modal's `done`. **The `true` is a
  change of mind about who is reporting**, that argument having been `streaming`: the rule was that
  silence is the default and a caveat overrides it, so a clean `Fetched` save said nothing, the
  screen already showing what it wrote and a note on every save being a note nobody reads. A modal
  that stays up until it is dismissed *is* the confirmation, and one that said nothing after a
  fetched export would be a dialog reporting silence. What is unchanged underneath is that a
  **blanked or cut** column speaks in either scope, naming the columns, because a user comparing the
  file to the screen needs to know which part of it to distrust. **`withheld` speaks again**, having
  been silent for a while on the argument that this grid already draws the cell as `<7 bytes>` — true
  of a `Fetched` save and false of the `PullChunks` scopes, whose rows were never on any screen
  (*core::export* has the rule and the wording; since every production caller asks with `true`, the
  clause is said here in both scopes); and the callback takes the sentence with `expect` rather than
  a fallback behind `unwrap_or_else`, `None` being reachable only from `streaming: false` — a second
  success message no input can produce is one no test can cover and one that drifts from the real
  one. The count is printed through
  `text::human_count`, the same row-count printer the
  stats line and the modal's own progress line use, so the figure in the report agrees with the
  `1k of ~16k rows` it replaces and with the stats line behind the backdrop —
  and the note shipped once as `Exported rows to employees.csv`, with no count at all, because
  `text::plural` returns the noun and nothing else and this call site simply never rendered the
  number. **The destination is not opened until the export has succeeded.** Both scopes write a
  `{name}.part` sibling (`export::part_path`) and `std::fs::rename` it over the target at the end,
  the dance `persist` already does for every config file and atomic *because* it is a sibling — a
  rename inside one directory never crosses a filesystem. It used to open the destination first, and
  `File::create` truncates, so a stream that died ten minutes in had already destroyed whatever the
  user was overwriting and all the bar could do was say so. The suffix is visible and
  self-describing rather than a hidden temp name, because when an export does fail the fragment is
  the one thing the user may still want: it is left behind and named in the message rather than
  swept away, deleting it being the one irreversible thing this path could do. **The streaming
  writer holds the cancellation token and refuses to publish when it is set** — a cancel reaches the
  writer as an ordinary end of stream, so publishing first and letting the reader declare the cancel
  afterwards would rename a truncated file over the user's own. Both notes say the two facts that
  follow from all of this: cancel is `Export cancelled — <name> was not changed; the rows that were
  written are in <name>.part` (`export_cancel_note`), and `export_failure_note` says the same in the
  failure's voice, where it previously said `<name> is incomplete` — true when the destination was
  truncated at `t = 0` and now the opposite of true. `Failed` carries `partial`, and `None` is not a
  formality: an export refused *before* the write starts (no connection, one already running) must
  not mention a file at all.
  **`partial` is read off `ExportFormat::writes_incrementally`, never written as `true`.** Excel is
  a ZIP and nothing reaches the sink until `save_to_writer`, so for it every pre-save failure left a
  **zero-byte** sibling that the sentence then pointed the user at; the failure arm now sweeps that
  empty file (`std::fs::remove_file`) and says nothing about it, and the cancel arm in `grid.rs`
  asks the same capability. **And a cancelled read now reaches the writer as a channel *error***,
  not as an end of stream. The two look the same to `PullChunks` — a cancelled read closes the
  channel — and for a buffered format that meant the whole discarded workbook was still assembled
  and compressed before anyone noticed: **measured at 12.3 s on 800k × 50**, with the bar still
  saying `Exporting…` and a Cancel that had already been pressed. Raising it at the pull returns
  `stream_to` at the next chunk instead, which is where the five incremental formats already stop;
  `a_cancelled_export_tells_the_writer_it_stopped` is the live-tier pin.
- **Every grid export raises a progress modal, and it stays up to say how the export ended.** The
  bar's `Exporting… Cancel` was static: nothing reported rows, so a large export and a stalled one
  looked the same for as long as either ran. `dump_view::export_progress_overlay`
  (there because it is the dump modal's twin — see that entry) shows the rows written and a red Stop
  while the write runs, then the outcome and a neutral **Close**, over
  `ExportUi { target, done, error, progress }` on `Ui`. **It reported progress only, and the exports
  that made that wrong were the fast ones**: the argument was that the bar told the whole story in
  one strip and a second reporter would be two places to keep in agreement, but a modal that closes
  itself when the write ends is raised *invisibly* by an export finishing in a frame or two — which
  is how the user found the uncapped path and read it as having no modal at all. So `target` stays
  `Some` after the write ends, which is what makes this a confirmation rather than a progress bar,
  and `done` / `error` carry the sentence; `Some` on either is what the footer reads to become Close
  instead of Stop, and only Close (or Escape) clears the four signals. **A cancel lands in
  `error`, not `done`** — stopping was the user's own doing and is neither success nor failure, but
  its sentence is about a file that is *not* what was asked for, and a green tick over it is the one
  reading it must not get. There is still no ✕, and **both exits run one `exit` closure**, the
  footer's button and Escape, which is `dump_overlay`'s rule that no two exits may disagree. It
  matters more here, because by state the two mean *opposite* things: an Escape wired straight to
  `target.set(None)` would silently abandon a running export, leaving a stream writing its `.part`
  sibling with its Stop nowhere on screen. **Both scopes raise it, with no size threshold.** It was
  `ExportScope::AllRows`' alone, on
  the reasoning that a `Fetched` save renders rows already in memory in one chunk with nothing to
  report from — sound about the code as it stood, and it still produced a feature the user could not
  find: that scope exists only when a result is *row-capped and re-runnable*, so an ordinary save
  showed nothing at all. The fault was justifying a limit by an implementation detail instead of
  changing the detail, and the detail changed: the `Fetched` arm renders through
  `export::SliceChunks` now, so it has a count and a Stop like the other. There is deliberately no
  row count to gate on either, though **not for the reason first written here**: "a small export
  closes the modal within a frame or two, which makes it self-regulating" described a modal that
  closed itself, and that self-regulation is precisely what made a fast export look like no modal.
  The reason left is simpler — the modal is the confirmation, so no export is too brief for it to
  serve, and any threshold would be a number nothing could justify.
  **`ExportTarget::total` is the denominator each scope can honestly give, and `approx` says which
  it is.** A `Fetched` export knows exactly how many rows it holds, so `1,234 of 5,000 rows` is a
  promise it keeps and carries no `~`; an `AllRows` export is re-reading the server and has only the
  catalogue's guess — the figure the `~All rows (M)` entry was named after, threaded from
  `export_menu` through `save_export` — so it reads `40k of ~180k rows`, is never divided by, and is
  not a ceiling: a real count larger than it is ordinary. `save_export` sets `total` as
  `if streaming { estimate } else { Some(fetched_total) }` — `fetched_total` being the rows of
  every block, since an SQL file of pending ＋Rows is several (`ExportRequest::more`) — and `approx`
  as `streaming`, and `approx`
  is **carried rather than inferred from the scope** because by the time the modal draws the scope is
  gone. Both sit on the *target* rather than on each progress message because the writer that counts
  rows has never heard of the catalogue, so a message carrying the field would carry `None` and wipe
  the estimate on the first block. `progress` is
  `None` until the first block, which the modal draws as an animated `Starting` rather than as
  `0 rows`, a stalled export and one that has genuinely written nothing being indistinguishable
  otherwise. **Only the export that raised the modal may write into it** —
  `grid::export_modal_closes(current, finished)`, comparing `ExportTarget::run` against the reporting
  run's `u64`. It took an `ExportRun` and asked only about *closing*; **reporting** is the operation
  that needs guarding now that the modal outlives the run, so `save_export`'s outcome callback asks
  it first and returns early. The failure is a second export request, refused synchronously by the
  app's single cancel slot: its tail arrives while the first export is still writing, and without the
  guard it would overwrite the running export's progress line with its own refusal and take that
  export's Stop off the screen for exactly as long as it mattered. `None` — no modal up — is `false`,
  there being nothing to own. `only_the_export_that_raised_the_modal_can_report_into_it` pins it, and
  `export_ids_never_repeat` pins what it rests on: a counter that repeated would hand one export
  another's modal. **The count is
  emitted from the writer, not the reader** — both of `main.rs`'s scope arms, over one
  unbounded crossbeam channel into `export_progress`, a channel rather than a callback for the reason
  the dump's and the AI stream's are (`create_ext_action` is one-shot and built on the UI thread,
  this fires once per block). In the streamed arm that is a decision: counted where the rows are
  *written* because the channel between reader
  and writer is bounded, so a reader-side count would run two blocks ahead of the file, reporting
  rows that are not in it yet and being two blocks wrong about what survived a cancel.
- **The results strip shrinks its prose, never its controls.** It is one flex row — stats, the
  read-more offer, the pager, the two red notes, a spacer, then the commit/row/AI/chart/copy/save cluster — and a
  flex row squeezes its children before it overflows, so whichever child refuses to shrink is the
  one that wins. The description lost that fight by default: at a 200k cap on a ~292k table the
  line was long enough to push the icon cluster off the right edge of a narrow panel, and the user
  saw a sentence where the export button should be. The rule is now explicit — `min_width(0)` +
  `text_ellipsis` on the stats label and on both warnings, `flex_shrink(0)` on the icon cluster and
  on the read-more label and the pager. A control clipped in half is not a control, while a description ending in
  `…` is still one; and the read-more offer counts as a control, not as prose, even though it is
  made of words. The `dyn_container` wrapping the sort caveat needs the `min_width(0)` too — a
  wrapper sitting at its min-content width holds the text at full size however the child is styled.
- **The RESULTS title bar carries Properties, Live Monitor and the editor-collapse toggle**, in that
  order and all tooltipped. Properties leads because it describes the table as it stands while the
  monitor watches it change — the same order the schema tree's Read group puts them in. Both act on
  the **shown result's** table — `shown_table`, `export::result_table` over the active panel's
  `QueryState::Loaded` result — not the tab's sticky `source`, which survives any edit of the SQL
  and had them describing and watching `users` under a grid of `SELECT * FROM orders`. With no
  loaded result `source` answers, having nothing to contradict it; a join, or a computed column
  beside real ones, answers nothing and both buttons dim rather than guess. They are gated on that
  table *existing*, which is deliberately weaker than the row
  actions' `insert_target`: a table with no usable row key passes, and the monitor then answers "No
  row key for this table" rather than the button being silently dead. The same reasoning covers a
  view, whose properties panel says which figures an engine publishes for one.
- **Find (Ctrl+F) and Go to row (Ctrl+G)** are two popups sharing one anchor at the panel's
  top-right, and both are **split in the same way**: the bar renders at the RESULTS-*panel* level
  (`grid_find_bar`/`grid_goto_bar`, mounted in `results_section`) so it can sit at the panel's edge,
  while the work happens in `grid_view`, which is the only place that has the row data. Find is
  incremental on `find_query`; goto fires on a `goto_step` **nonce** the popup bumps on Enter,
  because a jump belongs to submit rather than to every keystroke.
  **The find *jump* is budgeted, and `budget` is not an optional argument to `next_match`.** The
  jump runs on every character typed into the box — its effect's own comment says "incremental as
  the query changes" — and a needle that matches nothing, or whose early prefixes match nothing,
  walked every cell: ten million of them at the 200,000-row cap × 50 columns, synchronously on the
  UI thread, per keystroke. Its sibling the match *count* is debounced **and** capped at
  `FIND_COUNT_CELL_BUDGET`, with a comment saying the scan is "a String per cell" and must stay
  "well under a frame's worth of jank"; the jump, which is the expensive half, had neither. Running
  out means the jump does not move, which is what "no match" already looks like, and the readout
  says `N+` off the same budget so the two surfaces agree about where they stopped looking.
  **They shared the budget's *value* and not its *window*, which is how a counted match came to be
  unreachable.** `find_hits` walks row-major from display row 0 and stops after `budget` cells;
  `next_match` walked `budget` cells from the **caret**, wrapping. Past the budget the two only
  overlap: a 12-column result at the 200,000-row cap is 2,400,000 cells, so the count covers rows
  0..166,666 while a caret at row 100,000 covered 100,000..199,999 and then 0..66,666 — and a single
  match at row 80,000 was counted and could not be reached, Enter, next and prev all doing nothing
  while the bar said there was a match. The mirror case read `0/N+` with the caret sitting on the
  matched cell. The walk is over `0..budget` **itself** now — the count's window, exactly — starting
  at the caret within it and wrapping inside it, so a cell past the budget is neither counted nor
  jumped to, which is the agreement the paragraph above claims. `find_hits_within(cells, q, budget,
  hidden)`
  is the sibling that makes that seam testable: with the budget a constant, the only grid a test can
  write is one small enough for the two windows to coincide — which is the one configuration in
  which their disagreement is invisible, and is what all three of the existing budget tests used. It is
  split out of `grid_find` for the reason `find_hits` already is: the decision is testable without
  a live grid, and a budget nothing asserts is a constant nobody would miss. The cells themselves
  are read through `edit::GridCells::with_text`, which is what took two heap allocations per cell
  off that walk.
  **A hidden column's cells are passed over by both, and still spend budget.** The find bar searches
  what is on screen, so neither the count nor the jump lands on one
  (`a_match_in_a_hidden_column_is_neither_counted_nor_jumped_to`) — but the count charges a hidden
  cell to its budget all the same, so its window stays the linear `0..budget` that `next_match`
  walks. Skipping one for free would stretch the count's window past the jump's, which is the
  disagreement above over again. The grid calls `find_hits_shown` with its hidden set; `find_hits`,
  nothing hidden, is `#[cfg(test)]` now.
  **`0` in that readout means two different things, and only one of them is `0`.** `find_pos`'
  binary search answers `0` both when the caret is not on a match and when it is on one the
  *collection* never reached, and the second is still reachable through the other budget:
  `find_hits_within` also stops at `FIND_MAX_HITS`, while the jump's window is the cell budget alone,
  so past 100,000 matches the caret can land — correctly, on a highlighted cell — beyond where
  collection stopped, and the bar read `0/100000+` over a match the user can see. `find_readout`
  renders that case as `–/N+`: a readout that admits it does not know is better than one
  contradicting the grid beneath it. With `more` false the count is complete, `0` really does mean
  "not on a match", and the familiar `0/N` is right.
  `grid_view` keeps at most one of
  the two open, as the editor does with its own pair — in **both** directions: the exclusion
  tracked `goto_open` alone, so Ctrl+F over an open Go-to-row left both mounted on one anchor and
  you typed into the one you couldn't see. Go to row resolves through the pure
  `model::goto_row_index` — 1-based, in **display** coordinates (the gutter numbers what is on
  screen, so "row N" means the Nth row *as sorted*), over the rows the gutter **numbers**: a
  pending unsaved row reads `*`, so counting them in made "row 101" land on a row showing no number
  and made the clamp stop one short of the last one that does. It
  **clamps** to the nearest end when the number is outside the grid: past the last row goes to the
  last, `0` goes to the first, and a number too wide for a `usize` clamps with every other overshoot
  rather than falling through to the not-a-number path. **An empty integer part is the other way
  `parse` fails, and it is a zero rather than an overflow** — the branch's own comment is about a
  number too wide for the machine, and `.5k` took it: the suffix set the multiplier,
  `split_once('.')` yielded `("", "5")`, the emptiness test rejects only when *both* halves are
  empty, and `int_part.parse().unwrap_or(u128::MAX)` then saturated the count so a `.5k` in a
  200,000-row result jumped to the **last** row instead of row 500. The clamp is what made it
  silent, the box doing something plausible rather than nothing;
  `goto_row_index(".5k", 200_000)` answers `Some(499)` now and answered `Some(199999)` before. A row of 9s is how people ask for the bottom
  of a long result, and a silent no-op there can't be told apart from a broken feature, while
  overshooting is cheap to recover from — the gutter number and the row highlight say where you
  landed. **It accepts exactly the forms the grid prints**, which is why it isn't a
  `parse::<usize>()`: `human_count` writes `200k`, and typing that back returned `None` — the count
  on screen was a miss. `200k`/`1.25k`/`3m`/`1b` all read, in fixed point, the suffixes coming from
  `text::COUNT_UNITS`, the table `human_count` prints from (both under `text.rs`, with the test
  that pins the round trip); separators are accepted
  for a count pasted from elsewhere (the app writes none). **Both marks are the interface
  language's** (`grid_view` passes `i18n::number_marks()`), since the stats line prints its counts
  in them: in German `,` is the decimal point and `.` a group, so an English `1,250` pasted there
  is a fraction without a suffix and a miss, as `1.250` is in English. `None` is left for the only two cases
  that can mean no row: an empty grid, and input that isn't a number.
  The three decisions around it are **`model::goto_target`** — which row, the landing gesture, and
  the scroll column — so `grid_view` is a wrapper rather than a place any of them can quietly
  change. The gesture is `model::row_selection`, shared with the gutter click so the two can't
  drift: a divergence would also stop the aggregates bar reading the jump as a *row* and start it
  summing ids, which is why one test asserts that agreement across the crate boundary. Both take
  the drawn `edges` rather than `ncols`, so the row spans only the columns on screen
  (`a_row_gesture_spans_only_the_drawn_columns`). `scroll_col` is the **first drawn column**
  (`edges.0` — 0 until that one is hidden), not the active cell's column, so a jump doesn't also
  fling the viewport to the far right of a wide result. `goto_fires` is the first-run nonce guard (the effect is created whenever
  the grid is, and must not jump on its build run), and `one_bar_at_a_time` the find/goto exclusion
  — which is a **reactive** test over two signals in a `Scope`, the pattern to reach for when a
  rule is genuinely about signal propagation rather than about a value.
  **Closing either bar hands the keyboard back** (`focus_id.request_focus()`, on a true→false edge
  of the open flag). This is not optional and it is not the bar's job: Escape only flips the flag,
  floem then disposes the field's view and clears `app_state.focus` **silently**, and the grid was
  left focused on nothing — the next Ctrl+F reached nobody until the user clicked a cell. The bar
  can't do it either, being built a level up where `focus_id` doesn't exist, which is why the rule
  lives on the flag in `grid_view`. A new panel-level bar over the grid inherits this obligation.
- **Every control on a panel toolbar carries a tooltip too** — the schema panel's eye
  ("Show or hide databases") and gear ("Schema options"), the AI panel's new-chat and gear, Query
  History's trash ("Clear this connection's history…", named for what it clears, since a bare
  "Clear history" beside a per-connection list reads as all of it), and the terminal's
  open-DB-CLI / restart / gear. Server Activity had them already. Each is `.tooltip()` chained
  *after* the control's own `.style()`, for the reason the clock documents at `interval_button`:
  `.tooltip()` wraps the view, so a style applied after it lands on the wrapper while
  `on_move`/`on_resize` stay on the padded container inside — and that box is what the dropdown
  hangs off.
- **Every control on the results toolbar carries a tooltip**, and where it sits is a correctness
  rule. The nine are commit ✓, discard ✗, ＋, －, clone, ✦ AI, chart, copy and save (the pager's two
  chevrons, outside the ring, carry one each as well); before they were added
  nothing in the strip was labelled but the commit count, which is a bare number saying neither what
  it counts nor what pressing it does, and the glyphs doing the least reversible work (discard,
  delete, AI) are among the least self-describing. The tip goes on the **face, before
  `in_strip_button` wraps it**: `.tooltip()` allocates a fresh `ViewId` and it is the *wrapper* the
  ring registers and paints the focus outline on, so decorating the wrapper puts an id in the ring
  that paints nothing (the hazard `row_button` documents — see *Buttons are in the ring too*). The
  commit tip is built from that rebuild's count and busy flag rather than read reactively, since the
  `dyn_container` around it already keys on both and replaces face and tip together; the AI tip *is*
  reactive, because `ai_busy` dims its sparkle without rebuilding the block and a tip still offering
  to generate would be the only thing on screen disagreeing with the greyed glyph. Copy is
  deliberately **not** labelled Ctrl+C: that key copies the selection straight to the clipboard,
  which is a different action from the format menu the icon raises. `－` and clone keep their tip
  while dimmed and it names the selection, which is also the answer to why they are inert. Copy and
  save keep theirs while dimmed too, in an empty-result form — "Nothing to copy — no rows" /
  "Nothing to save — no rows", and chart's "Chart the results" becomes "Nothing to chart — no
  rows": a dimmed icon says only that it cannot be used, the tip says why, and
  without the second half the only reading left is that the app is broken.
  `toolbar_sep` takes 8px of horizontal margin on top of the cluster's own 3px gap: the icons carry
  a padded hitbox and no visible edge, so a divider set at the plain group gap reads as *part of*
  the group beside it rather than the boundary between two.
- **The three dropdown icons toggle.** A second press on AI, copy or save closes the menu it opened
  instead of dismissing and rebuilding an identical panel — the mechanism, and the ordering rules
  that make it correct, are under *Popup menus* (`popup_anchor` carries identity).
- **Row actions: new / clone / delete.** Gated on a single writable table (`EditModel::insert_target()`;
  hidden for joins / read-only), committed in the shared `GridWrite` transaction (`commit_writes` runs
  **deletes → updates → inserts**, each exactly 1 row). A keyless SQLite table opened through its
  rowid qualifies as writable here too, and the rowid column stages nothing: it is never editable,
  so it is absent from a new or cloned row by construction rather than by a rule anyone has to
  apply. **"Single" means the result's *origin* tables, not its keyed ones**, and it did not for one
  range: a join whose second table has no usable key read as a single-table result, so Delete row
  was offered on a row of the join and deleted from the keyed table alone — which affects exactly 1
  row, so nothing downstream could catch it. The gate is the only line here; `core::edit`'s
  `insert_target` entry has it in full.
    - **New (INSERT):** toolbar **"+ Row"** appends a blank pending row (`gs.new_rows`), rendered below
      real rows with a `*` gutter marker + faint green wash, first editable cell opened. Cells stage
      via `stage_new` (unset = server default; `Some("")` clears to default). Unset cells preview
      `<auto>`/`<required>`/`<null>`/`<default>` (from wire `auto_increment`/`no_default`/`not_null`).
      Tab/Enter hop cells (`advance_edit`).
    - **Clone:** right-click **Duplicate row** seeds a pending row via `add_cloned_rows`, which is a
      loop over `edit::cloned_row` — every editable column as the **grid is showing it**, staged
      edits included, except auto-increment and the binary columns whose displayed `<n bytes>` is a
      placeholder rather than a value (`core::edit`). **Past `edit::DUPLICATE_CONFIRM_FLOOR` (500)
      it asks first**, and where it asks is the load-bearing part: the question sits in the action,
      `clone_rows`, not on the gutter menu entry, so the cell menu's *Duplicate row* and the toolbar
      clone icon — both arriving through `clone_row` — cannot route around it, and neither can a
      fourth caller written later. `stage_cloned_rows` is the staging half past the question, and
      the `Confirm`'s resolver re-checks `gs.alive()` before reaching it, the modal outliving the
      click. The modal is the window's: `GridCtx`/`GridState` carry a `confirm` field holding
      `ui.overlay.confirm`, the same one the destructive actions raise.
      The gutter menu acts on the whole selection, so Ctrl+A on a large result is one click from
      building a `HashMap` per row on the UI thread; a floor rather than a cap because the count is
      in the entry's own label. `grid.rs`'s source gate
      `a_large_duplicate_is_confirmed_before_a_single_row_is_staged` asserts the ask precedes the
      staging call and that `clone_rows` does not reach `add_cloned_rows` itself.
    - **Delete:** right-click **Delete row** or the **Del** key marks a real row (`gs.del_rows`) with a
      red wash; marking clears its staged edits. `build_deletes` keys each `RowDelete` by the table's
      `key_cols` + original values. **A *modified* Del is nobody's**, and that arm says so with
      `m.is_empty()`: `grid_key` is not only the grid body's handler — the results strip hands it
      every modified key it declines (`widgets::strip_defers` is `!mods.is_empty()` minus
      ArrowLeft/ArrowRight/Escape) — so with the keyboard on a toolbar button **Ctrl+Delete staged a
      row deletion**, and Shift+Enter opened the in-cell editor. Neither is a binding the strip
      advertises, and before the strip began forwarding neither reached the grid at all. The
      plain-Enter arm carries the same guard (the arm above it claims Ctrl), and
      `the_staging_key_arms_refuse_a_modified_press` reads `grid_key`'s own span for both — a source
      gate because the handler needs a `GridState`.
  Inserts/deletes change membership/order → those commits **full-re-run** the query (pure-UPDATE
  splices in place). A NOT-NULL-no-default omission or duplicate-key clone fails the transaction and
  surfaces the error — nothing half-applied.
- **A write that waits says so.** A commit can block indefinitely on another session's row lock,
  and the grid used to sit there silently until `innodb_lock_wait_timeout` (50 s) turned it into a
  bare error. Both write paths therefore `arm_wait_note` when they hand the write off: if it is
  still in flight `WRITE_WAIT_MS` later, `tx::write_wait_note` fills `commit_wait` and the
  panel-level bar (`grid_error_bar`, the same bar as a commit error — a write is either still
  waiting or has failed, never both) states the wait and, when exactly one of the user's own
  transactions could be the holder, offers **Roll back _tab_** straight into `rollback_tx`. That
  last part is why the note is worth having: Schemaic owns both transactions, so it can name the
  user's own second tab where every other tool can only hang. The bar is deliberately **not**
  bounded like `run_ddl`'s `lock_wait_sql` — a timeout is right for a modal that refuses every
  exit, wrong for a cell edit that could just have waited a moment longer. `commit_seq` is what
  keeps an earlier commit's timer from narrating a later one.
- **A grid commit is deliberately not cancellable, and the shape of the code is the statement.**
  `main.rs` mints the commit's `CancellationToken` *inside* the `handle.spawn`, where nothing
  outside the spawned future can ever reach it to call `.cancel()`. Every other long operation does
  the opposite — `script_token`, `import_token`, `dump_token`, `export_token` and the per-tab
  `tokens` map are minted *before* the spawn and parked in a slot a view's Cancel/Stop handler
  reaches — so a token minted after that boundary is one nothing can cancel, and that is the intent.
  The affordance for a write that will not return is the bar above: **Roll back _tab_**, which ends
  the *other* transaction holding the lock rather than the write waiting on it. **The parameter is
  not a stub**, which is the counter-intuitive half and the reason this is written down at all. Both
  commit paths honour the token in full — `Db::commit_writes` through `write_on`, which checks
  between staged statements and mid-statement, kills the query server-side, *awaits* the killed
  statement so the connection's result stream stays in sync, and then asks the server what an
  explicit `ROLLBACK` achieved; a Manual tab's through `Session::commit_writes`, whose savepoint and
  `classify_isolated` answer for what survived. That machinery is there because a
  cancelled-and-dropped write once left durable `MyISAM` rows under a report saying nothing was
  written — the measured account is under *Write-back is transactional…* in *Architecture
  invariants*. So wiring a Stop control here would need no `schemaic-db` change at all: only the
  token slot, the cancel closure and the button. It is a product decision not taken, not a gap in
  the plumbing, and a release review's `R1-L2-01` turned on reading it as the latter.
- **Type-aware headers** show `type_name` under the name (two-line, `grid_header_h()`). A sorted column's
  name + chevron use `chip_active()`; a column with selected cells gets a `grid_col_sel()` header
  background.
  **Which sort the chevron draws is `header_sort_indicator(server_dir, client, ci)`, and it is read
  through a closure rather than computed in `header_cell`'s body.** A filter/sort-eligible result
  keeps its order server-side in `grid_query`; everything else sorts the loaded page in memory. The
  two are meant to be mutually exclusive, and the pure function prefers the server's answer outright
  rather than combining them, so that premise never has to hold for the header to be right
  (`the_server_sort_outranks_the_client_one`). The read has to be a closure because
  `gs.server_sort_dir(ci)` tracks `grid_query` while `header_cell` is called from the body's builder
  and again from the virtualised header row's, and neither key mentions `grid_query` — a builder is
  not a tracking scope, so the value computed there subscribed to nothing and the chevron froze.
  **It was reachable, not theoretical**: `cycle_server_sort` hands its new order to
  `apply_grid_query`, which used to write it to `grid_query` and *then* take one of three exits
  that set `view_err` and re-ran nothing — `build_query`
  answering `Ok(None)` on a statement it cannot rewrite, a `FilterError`, and the write guard's
  `RerunRequest::approved` refusing. All four refusals, `filter::requery`'s staged-work one
  included, now come **before** the write, so a refused sort leaves the chevron on the order that
  is still on screen; the closure stays because the chevron must follow the write a dispatched
  re-run makes before its rows land. `filterable()` only asks that `base_sql` and
  `insert_target()` are both `Some`, never whether an `ORDER BY` can actually be spliced in, so the
  gap is the join shape `edit::insert_target`'s own doc is written around —
  `SELECT o.* FROM orders o JOIN order_lines l ON l.order_id = o.id`, where every column has an
  `orders` origin so `insert_target` is `Some`, while `build_query` rejects the join. Clicking such
  a header left the bottom bar saying the query cannot be filtered while the header went on drawing
  the order before last, disagreeing with a `grid_query` that had already changed; a *successful*
  re-run hid the whole class by rebuilding the grid from a new `ResultSet`. The chevron slot itself
  is a nested `dyn_container` over that closure, because sorted and unsorted are different children
  — the general rule is under *Floem 0.2 gotchas* — and
  `a_server_sort_read_sits_in_a_tracking_scope` scans this file so the read cannot migrate back out
  of a closure.
  **Key icons** (PK = gold key-round, single-col index = blue key-square, FK = purple
  key-square; colours shared with the schema tree via `key_primary/index/foreign`) come from
  `column_key_map`, cross-referencing the tab's `source` against the loaded schema (`db_nodes`). Only
  populated when the tab was opened from a table with schema loaded; arbitrary SELECTs get none.
  **Nullable markers** need none of that: a faint 10px ∅ (`icons::CIRCLE_SLASH_2`,
  `theme::text_faint()`, a *Nullable* tooltip) follows the name of any column for which
  `Column::is_nullable` holds, which is on the `ResultSet` from the moment it arrives — table tab or
  arbitrary SELECT alike, no `db_nodes`, no `key_gen`. It sits on the **name line**, between the
  name and the sort chevron, and deliberately not beside the key icon at the left: a leading icon
  gives up the right-alignment of a numeric header, as a key column's already does, and a marker
  that most columns carry would take it from most headers. **It means "this result column may be
  NULL, as the engine reports it"**, and the engines report from different places, so it is
  stated per engine rather than made uniform: MySQL and MariaDB answer for the result — an outer
  join's `NOT NULL` column is marked, a view keeps its base's `NOT NULL` — and PostgreSQL for the
  catalog row the column resolves to — an outer-joined `NOT NULL` column is not marked though its
  row can hold NULL, and a view's columns all are, since the catalog carries no `NOT NULL` for
  them. It used to be documented as the base column's declared nullability, which was true on
  PostgreSQL alone; making it that on MySQL would take a catalog read per result for a faint
  icon. The live tier pins both directions on each leg (`nullable_mark_is_the_engines_answer`,
  over `Target::nullability_is_the_results`). An expression column carries no marker because it
  has no answer to report. **The width
  estimators budget for it**: `init_widths` and `autofit_width` both start from
  `header_name_chars(col)` — name + 3 for the chevron + `NULL_MARK_CHARS` (2) when marked — so they
  size the header `header_cell` actually draws; left out, the marker pushes the name into clipping
  on exactly the columns whose widest text is the name
  (`a_nullable_columns_name_line_budgets_for_its_marker`). The 2 is exact only at the default font
  size — `icons::icon` sizes the glyph by `scaled_font`, the budget is in `scaled` grid characters —
  so another size moves the icon without moving the budget.
- **Column virtualization.** Both rows (`virtual_stack`) *and* columns are virtualized: the header and
  every data row render only the columns intersecting the horizontal viewport (+ a small overscan)
  between two width-preserving spacers, so a wide table builds ~10-14 cells/row instead of all of them
  (a 100k×50 inertial fling stays smooth). The visible window is a `ColWindow` (`start..end` into
  `data_cols` + left/right spacer px) from a `create_memo` — it recomputes on scroll but, since memos
  dedup on `PartialEq`, only *notifies* (rebuilding header + row cells) when the visible column set
  changes, not every pixel. Header and every row read the **same** `win` memo, so the panes stay
  aligned. Invariant: `gs.widths` stays full-length and each row's total width = `sum(widths[data_cols])`
  (spacers make up the hidden columns), so `h_off`/`scroll_to` geometry is unchanged —
  `scroll_active_into_view` sums in data-pane space (`ColLayout::scrolling`, which excludes the
  frozen columns and the hidden ones) to match.
