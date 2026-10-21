# Popup menus (`menu_panel`)

Custom themed overlays, not Floem's native `Menu` (native renders OS-styled, clashes with the dark
theme). `menu_panel(entries: Vec<MenuEntry>, close)` takes `Action`/`Sub`/`Separator` entries and
renders the themed panel; the caller positions it absolutely. Used by the schema right-click menu
(`context_menu_overlay`) and by every other popup through `popup_menu_overlay` — which makes it
every `<select>` built on `settings::in_ring_picker` too. **Three dropdowns are not `menu_panel`s**:
the QUERY toolbar's database selector, the header's connection switcher and the SCHEMA gear's
database list build their own panels in `overlays.rs`, so nothing below holds for them — no edge
flip, no keyboard cursor — and their height cap is `widgets::room_below`, the room under their
anchor, rather than the window-wide one `menu_panel` applies (GitHub #21; the `overlays.rs` entry).

- **Separators are tidied by the panel, not the builder** (`tidy_separators`, applied first thing in
  `menu_panel` and by `menu_panel_height` so the measured panel matches the drawn one): leading and
  trailing separators are dropped, a run collapses to one rule, and each submenu's own children get
  the same treatment. A builder pushes a group's separator *before* it knows whether the group has
  any entries — the column menu pushes one and then asks `field_entries` whether Edit column and
  Drop are offered, and on a **view's** column neither is — so the alternative is every conditional
  arm remembering to push its rule afterwards. It shipped as a rule with nothing under it: an empty
  section between "Copy qualified name" and AI explain.
- **Nested submenus**: a `Sub` entry hover-expands a child `menu_stack` anchored to the parent row's
  right edge. The **keyboard** stops at one level (`MenuLevel`/`MenuSub`), which is as deep as any
  menu in the app goes; one flat pair of cursors is what lets `menu_key` drive whichever level is
  open without walking a tree it would then have to keep in step with the views.
- **The open submenu is drawn at the root of the window, not under its row** (`submenu_layer`, the
  last element of `workspace`'s root stack, after even `popup_menu_overlay`). It has to be: nested
  under its row it was painted but never hit-tested whenever it flipped left or shifted up, because
  Floem grows a parent's hit area rightward and downward only (see *Floem 0.2 gotchas*). It opened,
  it drew, and it answered neither hover nor click. That went unnoticed for as long as it did
  because it bites only near a window edge, and it surfaced on the ER diagram's export menu, which
  is anchored at the right end of a toolbar and so flips on any window narrow enough.
  A `Sub` row therefore builds no panel. It publishes `OpenSubmenu { entries, row, level, close }`
  into the `hoisted_submenu()` channel — a thread-local signal on a **detached scope**, since what
  publishes is a row inside a panel that is disposed the moment the menu closes — and the layer
  draws from that. Publishing is a `create_effect` on `open_sub`, not something `PointerEnter` does,
  because the keyboard opens submenus through the same signal (`MenuAct::Open`); one place, both
  ways in. **Clearing is `menu_panel`'s job, not a row's**: every row's effect re-runs on every
  change, so a row that also cleared would race the row that is opening. `menu_panel`'s effect fires
  on `open_sub == None`, which also sweeps up a stale submenu as it opens; `workspace` clears from
  the other side when both menu channels go empty, which is the click-away case where the panel's
  whole scope is dropped and its effect never runs again.
  Two things depended on the submenu being a view-tree descendant, and both survive the hoist:
  dismissal (the panel absorbed its children's pointer-downs — `menu_stack` does that for the
  hoisted copy too, being the same view) and the parent row's highlight (the keyboard cursor sits on
  that row while its submenu is open, and the cursor wears the hover fill).
  The layer is out of flow and **shrink-wrapped to the panel**, deliberately not a full-window
  sheet: a full-window layer would claim every pointer event in the window and swallow clicks meant
  for the app underneath.
- **Hover intent (no timers)**: entering a leaf clears `open_sub`, entering a submenu row sets it;
  nothing closes on leave. The submenu is flush with the panel's right edge, so a diagonal move never
  crosses a gap — the close-on-diagonal problem is avoided structurally.
- **Dismissal**: the panel `on_event_stop`s its own pointer-downs, so the root "pointer-down anywhere
  closes" handler (in `workspace`) fires only for outside clicks. Escape and any action also call
  `close`. A hoisted submenu is a sibling at the window root, not a descendant, so the panel absorbs
  nothing on its behalf — it stops its own pointer-downs, `menu_stack` carrying that
  `on_event_stop` for both copies because it *is* the same view.
- **Edge-flipping**: submenus flip left past the right edge and shift up past the bottom — from the
  parent row's window position (`on_move`/`on_resize`) + the live `window_size()` global (set from
  `workspace`'s root `on_resize`). `popup_menu_overlay` flips the whole panel the same way at the
  cursor. **Tracking `window_size()` means the placement closure re-runs on a resize, so what it
  resolves there has to be only the flip.** The schema context menu's anchor is now a `create_memo`
  over `ctx` alone: the closure used to re-evaluate `menu.at.unwrap_or_else(|| last_mouse
  .get_untracked())` on every run, so a border drag or Win+Up teleported a menu raised on a row at
  the top of the tree to wherever the pointer had since wandered. Reading `last_mouse` untracked
  stops a bare pointer *move* from re-placing the panel and does nothing about a re-run driven by
  another signal; the keyboard route was immune only because it fills `at`. Fixed at the open,
  re-flipped on the resize — which is the half that should follow the window. Only the width is a
  flat estimate (`SUBMENU_FLIP_W = 210` for a submenu; the popup uses the
  panel's real `min_width`); the height both ask for is `menu_panel_height`, which sums the entries
  that will actually be drawn — 30.5 per action row, 9 per separator, plus 14 — because counting
  separators as full rows shoved an upward-flipped panel tens of px too high. A row carrying a
  **detail** line (`MenuEntry::action_detail`, the faint second line the terminal picker's shells
  wear so `pwsh.exe` and `powershell.exe` are told apart) is the one row that is not one line tall,
  and both estimates know it: the height adds `MENU_DETAIL_LINE_H + MENU_DETAIL_GAP` and
  `menu_panel_width` takes the **wider** of the two lines, measuring the detail at `font_label()`
  where the label is measured at `font_title()`. Under-predicting is the dangerous direction — it
  says a panel fits below its anchor when it does not — which is why both are pinned at Normal and
  at Huge. **How that 30.5 is
  split matters away from 100%**, because `scaled()` is applied to each term separately: it was
  `MENU_LINE_H` 18.5 over a `MENU_ROW_PAD` of 6, while the estimate's own doc said 14 + 8 + 8 and
  `menu_row` *drew* a `font_title()` line inside `padding_vert(scaled(8.0))` — three decompositions
  of one row, all landing near 30.5 at Normal, which is why nothing caught it. The padding half is
  now the drawn one (`MENU_ROW_PAD` is 8.0 and `menu_row` reads that very constant, so the estimate
  and the row cannot state different numbers again) and `MENU_LINE_H` is 14.5, the residual of the
  *measured* total rather than a line-height theory that disagrees with it. Neither is measured
  from a laid-out panel, so there's no open-then-flip flicker.
  A submenu's own flip is `submenu_insets`, which is a **pure function with tests** rather than four
  lines inside a style closure, because every way it can be wrong is silent — a sign slip or an `x1`
  where an `x0` belongs still draws a submenu, just not beside the row that opened it. It pins an
  *edge* rather than computing a corner: flush to the row's right edge normally, and when there is
  no room, `inset_right` from the window pins the panel's **right** edge to the row's left one
  whatever the panel measures. The width estimate therefore only ever decides *which* edge to pin,
  never where — so being off costs at worst a flip that wasn't needed, never a gap between the row
  and its submenu, and never an open-then-measure-then-move flicker. Same for the vertical: a panel
  that won't fit below the row pins its bottom to the window's, with no height arithmetic at all.
- **A keycap only where the key and the click are the same act on the same thing**
  (`MenuEntry::shortcut`, which takes a `shortcuts::MenuKey` — see `shortcuts.rs` under *Crates*).
  It is the palette's rule, for the palette's reason: a nearly-right keycap teaches a key that does
  something else. So the gutter menu's Copy wears Ctrl+C and the cell menu's Paste Ctrl+V (both land
  on the selection, as the keys do), but the cell menu's Copy wears Ctrl+C only when its `CopyScope`
  is `Selection` — the `Cell` scope copies the one raw value, which the key does not promise — and
  the gutter menu's Delete/Undo delete wears Del only when the right-clicked row is inside the
  selection, since Del marks the selection. From the gutter that is always so — its right-click
  re-selects a row outside the selection before it builds the menu, so a row number outside it
  shows Del too, and Del then acts on that row as the entry does; the test is for an opener that
  does not re-select.
  The cell menu's single-row *Delete row* wears none, and neither does the editor's *Ask AI*, which
  acts on the right-clicked statement where Ctrl+K takes the caret or selection — nor, for the same
  reason, *Format statement* and *Make single line*, whose keys act at the caret; the editor's
  *Format code* wears Ctrl+Alt+L, being the same `format_editor` call — and takes its label from
  the row too (`MenuKey::label`), so it is called what the shortcut sheet and the palette's
  *Format Code* call it (`the_format_action_has_one_name_on_every_surface`; it was "Format", "Format
  SQL" and "Format Code"). The builder is a no-op on
  `Sub`/`Separator` — a submenu row's trailing slot is its chevron
  (`a_shortcut_lands_on_an_action_and_nowhere_else`). `menu_row` draws the keycap after the same
  flex spacer (floor `MENU_SUB_SPACER`) a chevron uses; it is one line and shorter than the label,
  so it costs width and no height, and `menu_panel_width` adds the spacer, two `MENU_KID_GAP`s and
  `keycap_width` *beside* the label-and-detail column, to whichever of those two lines is wider. A
  width that forgot it clips the keycap at the panel's right edge — the one place a flip was
  computed to keep it — which `a_shortcut_widens_its_row_by_at_least_its_keycap` pins (seen red
  with the term zeroed). The keycap is `widgets::keycap`, the palette's extracted so the app has
  one; `keycap_width` measures it in the mono face at `font_label()` plus `KEYCAP_PAD_H`, the very
  constant `keycap` pads with, so the drawn and the measured cannot disagree. Its `tint` is applied
  to both colours, which is how a disabled row (or a faded palette row) dims its keycap with it.
- **Two menu channels**: the schema tree uses `ui.context_menu` (typed `CtxMenu`) +
  `context_menu_overlay`; everything else uses the generic `ui.popup_menu`
  (`RwSignal<Option<Vec<MenuEntry>>>`) + `popup_menu_overlay`. Both overlays live at the workspace
  root (window coords) and close on the root pointer-down. **Both render through `menu_panel`**,
  which is why everything below is stated once and holds for every opener. (It said "all fourteen"
  until the Live Monitor's Export made it fifteen. The count was never reproducible — the openers
  are *logical* menus, not `popup_menu.set` sites, several of which are helper-driven — so it is
  gone rather than incremented: the sentence needs "all of them", not a number that silently rots.)
- **Every schema-tree menu is a subsequence of one skeleton, and `Drop` is always the last entry in
  its menu.** `overlays::context_menu_overlay`'s `build` closure matches on `CtxKind` — one arm per
  kind of row — and each arm emits the same five groups in the same order, separated by
  `MenuEntry::Separator`, so an action sits in the same place whatever was right-clicked:
  **Open** (what a double-click would have done), **Read** (`Copy name`, `Copy qualified name`,
  then what the node can *show* you — `Properties`, `Used by`, `Live monitor`, `ER diagram`,
  `Generate DDL` —
  closing with
  `Refresh`), **Tree state** (`Favourite`, `Colour ▸`, `Hide`, which act on the row and not on the
  object), **Write** (`Create`/`Edit`/`Import`/`Triggers`, with the entries that can't be taken
  back **last** inside the group and coloured `theme::error`), and the `AI explain` row every menu
  ends with, appended outside the `match`. The write group's ordering is the load-bearing half: the
  row the cursor lands on after a right-click must never be the irreversible one, and two menus
  broke that before the skeleton was written down — the key/index menu opened straight onto
  `Drop index`/`Drop foreign key` with its edit entry (then labelled `Edit table`, now named for
  the row) *below* them, and the column menu's `Drop` was rendered in the same colour as the
  `Edit column` directly above it. A new entry joins its group rather than the end of the list, and
  a new arm reads the skeleton, which is stated as a comment
  block immediately above the closure: an arm is written and reviewed one arm at a time, and
  nothing else in the file says what the order is.
  **`overlays::menu_order_gate` holds the whole claim**, in four tests over the module's own source,
  bounded by the `let build:` binding and the `AI explain` row pushed outside the `match`. The
  ordering can't be reached through `build` — it closes over a `Ui` — and it doesn't need to be: the
  order is a property of the *source*, because a dialect or a read-only connection can only **omit**
  an entry, never move one, so one pass covers every engine and every permission state at once,
  including engines that don't exist yet. `nothing_follows_an_irreversible_entry_in_its_own_menu` is
  checkable at all because every destructive entry marks itself
  `action_colored(…, theme::error, …)`, the same fact the menu shows the user, and
  `the_error_colour_marks_the_drops_and_truncate_and_nothing_else` pins that the marking is on
  exactly `Drop`, `Drop foreign key`, `Drop index` and `Truncate` so re-colouring can't weaken it
  silently. `every_menu_is_a_subsequence_of_the_skeleton` carries the five-group claim: `group(label)`
  is the skeleton as data, and an **unknown label fails** rather than being skipped, which is the
  load-bearing part — a thirteenth entry can't be added to any menu without placing it in a group
  first, which is the drift the comment was written to stop and could not.
  **The gate reads message ids, not words.** Every builder label is `tr!("tree-…")`, so the first
  literal the scan meets is the id: `group` is keyed on ids, the colour test expects `tree-drop`,
  `tree-drop-foreign-key`, `tree-drop-index` and `tree-truncate`, the drop checks match the
  `tree-drop` prefix, and the gate holds in every language. The closing landmark is `AI_EXPLAIN`,
  the id **with its quotes** (`"tree-ai-explain"` as it stands in the source) — the quotes are what
  keep a comment naming the row from standing in for it, the trap the landmark already fell into
  once with the English label.
  `drop_is_the_last_entry_before_ai_explain` pins the one position that isn't a matter of taste, in
  both halves: every menu that writes ends on its `Drop` (`Database`, `Schema` and `ObjectGroup` have
  nothing to drop, asserted by name), and nothing but `AI explain` follows the `match`.
  The two deviations the shipped code already had stand, and the gate tolerates them **because they
  stay inside their group**, which is recorded at `group` rather than quietly excused: the database
  arm pushes `Collapse all` after `Refresh` where the skeleton closes the read group with `Refresh`,
  and the table arm's write group is `Import` → `Edit table` → `Triggers` where the skeleton lists
  Create/Edit/Import/Triggers. Order *within* group 4 is not checked; `Drop` being last is.
  `Back up` and `Restore from file` are group 4 beside `Import`. The restore is a write; `Back up`
  writes a file, like `Export` beside it, and the server only on SQL Server
  (`launch::backup_writes_server`), but sits with the node's round trips
  rather than taking `Export`'s skip, so an unknown-label failure still covers it.
  **The near-collision the table is load-bearing for is `Refresh` (group 2) beside
  `Refresh view` (group 4)**, which a materialized view's menu carries both of: the first
  re-introspects the *database* into the tree, the second re-runs the view's query on the
  server and replaces every row it stores. Nothing but `group` keeps them on opposite sides
  of the separator, and a `Refresh view` sorted into the read group would put a write where
  the eye expects a reload.
  **The gate reads `overlays.rs` and nothing else** — `this_file()` hard-codes the path — and two
  menus in the app are outside it, both of which then shipped the inversion it was written to catch.
  The SCHEMA gear put `Create database` (group 4) *above* `Users and privileges` (group 2), and the
  tree's blank-space menu did the same; the gear now runs Refresh · Collapse all · Users and
  privileges · Server variables · Create database · Show table sizes, and the blank-space menu
  carries its own half of
  the gate in `schema_tree::blank_space_is_a_subsequence_of_the_skeleton`, asserted over the entry
  **values** rather than over the source, since unlike the context menus those entries are data,
  and walked for every dialect in `SqlDialect::ALL`. It
  places each entry by calling `group` here with the entry's message id (`BlankKind::id`, test-only)
  — which is why `menu_order_gate` is a `pub(crate) mod` and `group` a `pub(crate) fn`. It used to
  mirror the numbers by hand, keyed on English labels against this table's ids, and a copy like
  that can disagree with the original in silence; now there is one table for every menu, and
  `every_blank_entry_label_is_its_id` holds the id to the message `BlankKind::label` actually
  draws, or the gate would be ordering one entry while the menu showed another. That is why
  `tree-users`, `tree-server-variables` (group 2 both, the second a read of the server like the
  first) and `tree-new-database` (group 4) are in this table at all: none is **a context-menu row
  today**, but an unknown id fails, so a second menu cannot place an entry this one has never
  heard of. The
  vacuity guard (`arms.len() == 7`) counts arms of the builder the gate already reads, so it cannot
  notice a third source either.
  **`Export` is the one label the gate skips outright**, and it is an exemption rather than a third
  deviation: it has two honest homes — with `Generate DDL` on a database or a namespace, among the
  read entries that hand you what the node holds; directly below `Import` on a table, where the two
  are the file pair — so no single group can hold it without making one of the two placements a
  failure. It is safe to exempt because neither placement can be wrong in the way this gate exists to
  catch: it writes a file and never the server, so it is never the irreversible entry group 4's
  ordering keeps off the cursor, and the position that does matter is still pinned by
  `drop_is_the_last_entry_before_ai_explain`. The skip is a `continue` on that one label inside the
  test, not a lenient `group` — an unknown label still fails, which is the part that stops drift.
  **A skip is only honest while the scan can still see the thing it is skipping**, and that stopped
  being free the day `Export` became a submenu built by `overlays::export_submenu`: the constructor
  moved into the helper, so without a branch of its own `built_entries` finds no `Export` in any menu
  at all, the `continue` never runs, the comment above it describes nothing, and the day someone
  gives `Export` a group there is no entry to check it against — a dead branch that still reads as
  live coverage. `the_scan_sees_the_export_submenu_in_all_three_arms` pins that seam by asserting the
  three arms it is found in, which is the assertion a test written against the scanner alone would
  have missed.
  Four things the scan has to get right, each of which made it wrong first: comments are cut out of a
  constructor's span (one entry explains its own label with `"(0)" reads as a broken count` two lines
  above the label, and the scan read the comment as the name); a label is read only from the span
  *before* `move ||`, which is what makes a twelve-line window safe; `let label = if … {"A"} else
  {"B"}` above a constructor is resolved backwards, since an empty label would have excused
  `Favourite` and the key row's `Edit` from the group check; and only entries pushed into `entries`
  count, because the colour swatches are `MenuEntry`s too — with `entries.extend(create_submenu(…))`
  counted as the `Create ▸` entry it is, or the two arms that write through it would look as though
  they stop at the read group, and `entries.push(export_submenu(…))` counted the same way for the
  same reason.
  The Table arm's `Truncate`/`Drop` name the **scale** of what they delete when a row figure is
  already in `ConnNode::stats` — Truncate in its confirm, Drop as an extra line under the preview's
  warning, since it no longer asks first (`stats::truncate_prompt`/`drop_warning` decide the words and
  whether a figure is worth naming). It is read, never fetched: the menu is built on the
  right-click, so a round trip there would either block it or land after the modal is already up.
- **Keyboard operation lives in `menu_key`.** The panel is a `focus_root`, so it took focus and
  answered Escape from the start — but nothing moved a cursor and no row was marked, so a menu
  opened with Enter from a ringed button could only be finished with the mouse and read as though
  it had never taken focus. Up/Down step `menu_stops` and **wrap** (the swatches' rule, not the item
  list's: a menu is short and its ends read as adjacent), Home/End jump, Enter/Space runs the row and
  closes, Right/Enter opens a submenu and lands on its first child, Left comes back out, and Escape
  closes an open submenu **before** the menu so "back" never skips a level.
  **Tab steps it too, and above all it is *claimed*.** The catch-all at the bottom of `menu_key`'s
  match returned `Continue` for Tab, and an unprocessed Tab reaches floem's last resort:
  `view_tab_navigation` over the **whole window tree**. A menu raised over the bare workspace has no
  ring beneath it — a `menu_panel` registers as a ring-less `focus_root`, which
  `widgets::innermost_ring_root` skips — so the workspace root's Tab backstop declined as well and
  focus landed on an unrelated control behind the panel. The menu then stood there keyboard-dead:
  ↑/↓/Home/End/Enter/Escape are all listeners on the panel, and floem delivers a key only to the
  focused view and then to `main_view`, so none of them reached it, the menu could be dismissed with
  the mouse alone, and Enter activated whatever the walk had found. It *steps* rather than merely
  swallowing, because a key that does nothing is its own small defect and a `<select>`-shaped popup
  steps on Tab anyway — and it asks `widgets::steps_ring`, which is the one predicate, so the
  `primary_held` term comes with it and `Ctrl+Tab` stays the app's next-query-tab rather than being
  eaten by whichever menu happens to be open. Two things the cursor
  must not do are why `menu_stops` exists rather than a range: resting on a **separator** makes Down
  look dead, and resting on a **disabled** row offers an Enter that silently does nothing. The
  pointer moves the same cursor (each row's `PointerEnter` sets it), so there is never a second
  highlight disagreeing with what Enter would run. `menu_key` takes only signals, so the whole
  decision asserts without a window (`widgets::menu_key_tests`).
- **A menu raised by a *button* must set `popup_anchor`.** With it `None` the panel opens at
  `last_mouse`, which is right for a right-click and wrong for a button: reached by Tab and pressed
  with Enter, the pointer is wherever it was left, so the menu opened across the window from the
  control that raised it. `table_designer::suggest_chevron` anchors to its own ring wrapper —
  `ViewId::layout_rect()` is already in window coordinates, the frame `PopupAnchor` is stated in.
  The grid's toolbar dropdowns and the status-bar segments were always right: floem's `on_move`
  fires during **layout** with the view's window origin, not on pointer movement, so what they
  anchor to is the widget and not the cursor.
  **Which placement it asks for is part of the same answer.** `BelowIcon` tucks the panel under a
  ~28px glyph by opening 40px left of the icon's *right* edge; measured from a 220px enum field or
  a grid cell that is most of the way across the control, so a menu dropping from a **box** uses
  `BelowBox` instead — left edges flush, the same right-edge flip, and a *vertical* flip that clears
  the control instead of covering it (`box_menu_inset`; `BelowBox` carries the box's top edge for
  exactly that, `BelowIcon` doesn't and deliberately overlaps its glyph). The two are
  distinct variants rather than a flag because the anchor is also the menu's identity: a control
  compares the placement it *would* set against the open one, and two controls that disagreed about
  which variant they use would each fail to recognise the other's menu.
- **`popup_anchor` carries the menu's *identity*, not only its placement — so an opener must write
  it immediately before `popup_menu`.** One channel serves sixteen openers across eleven files
  (`activity_panel`, `chart_view`, `widgets::open_picker` — which is now every `<select>` in the app, not only the
  cell editors' — `connection_form`, `editor_pane`, `erd_view`,
  `grid` ×6, `lib`'s status bar, `monitor_view`, `table_designer::suggest_chevron`, `tabs`) and nothing in it says who filled it, so the grid
  toolbar's AI / Copy / Save icons ran "dismiss, then open" unconditionally and a second press
  rebuilt an identical panel instead of closing it. The menus that already toggled could only half
  lend their answer: the schema panel's eye and gear, the connection switcher and the tab selector
  each render their *own* menu and so keep a private `RwSignal<bool>`
  (`db_menu_open`/`schema_menu_open`/`conn_menu_open`/`active_db_menu_open`), which is available
  only to a trigger that owns its panel. **`widgets::menu_anchored_at(open, anchor, mine)` is the
  answer for a trigger that shares the channel**, comparing `popup_anchor` against the
  `PopupAnchor` it would set itself — which is why `PopupAnchor` derives `PartialEq` and why its
  rustdoc calls that derive load-bearing rather than a convenience. It is self-invalidating **only
  because every opener overwrites the anchor as it opens**: there is no separate flag to go stale
  and nothing for the other ten to reset. An opener that fills `popup_menu` without setting
  `popup_anchor` first therefore hands its menu to whoever opened last, silently.
  **`widgets::popup_anchor_gate` is what catches that**, and it reads the source, because the sites
  are not greppable by one name — `overlay.popup_menu.set(Some(…))`, `gs.popup.set(Some(…))` and
  `table_designer`'s local `popup`/`anchor`. Walking each file of the crate in order, every write
  that *fills* the channel must have an anchor write between it and the previous fill: a stand-in
  for "in the same opener, before it", which holds because openers don't interleave. `rustfmt`
  breaks the long ones at the `.`, so the scan joins a continuation onto its owner — getting that
  wrong reported two correct openers as offenders. Deliberately weak, like `shortcuts`' `KEY_FILES`
  and `core/tests/doc_coverage.rs`, and it asserts a floor on how many openers it found so a rename
  can't make it pass by seeing nothing. Its `EXEMPT` — openers that deliberately write no anchor —
  is **keyed on the call text**, like `consts.rs`'s and `dividers.rs`', and used to be
  `(file, line, reason)` against a number taken from `production_code`'s output, which before that
  helper preserved the file's numbering bore no relation to the file at all. Even with the numbering
  right a position key moves whenever anything above it does, so an exemption would silently re-arm
  the gate on the site it was written for, or licence a different one. The array is empty, which is
  the healthy state and is why this was latent — and it was the documented way to add one. Folding the pair into one `open_popup(anchor, entries)`
  constructor, so the anchor *cannot* be omitted, is still the better end state and is a change to
  make with the app running.
  `open` (is the channel non-empty) is checked **before** the anchor, which is the reason this is a
  named function rather than an inline `&&`: closing clears `popup_menu` but leaves `popup_anchor`
  naming the last opener, so an anchor-only test reports the menu still up after Escape and the
  next press closes nothing instead of opening. Six tests in `widgets::menu_key_tests` pin it,
  including `a_dismissed_menu_is_no_longer_owned` and `a_cursor_menu_belongs_to_no_trigger` — a
  right-click menu sets the anchor to `None` on this same channel. Each trigger states its anchor
  **once** (`grid_toolbar`'s `anchor_below`, `status_menu_seg`'s `anchor_here`,
  `suggest_chevron`'s `anchor_now`) because the value that *places* the panel is the value that
  *identifies* it: written twice, a pixel of drift would open the menu correctly and silently refuse
  to toggle it shut. Recomputed on each press rather than remembered, so it is the *current* rect
  that must match — `suggest_chevron` sits in a scrolling modal body, and scrolling with its menu up
  moves the chevron out from under it, at which point reopening at the new position is the better
  answer than closing. Which way the test fails matters more than its exactness.
  A tag beside the channel is what this replaced, and why it isn't the pattern: the status-bar
  segments carried a `menu_owner: RwSignal<u8>` written only by the segments themselves, so it went
  stale the moment anything else filled the channel — open a segment's menu, right-click a grid
  cell, press the segment again, and it closed the cell's menu instead of opening its own. All three
  surfaces ask the anchor now.
- **The app's menus are mutually exclusive, and the list of them is `widgets::MenuFlags`** — the
  eight `MenuId`s (`Popup`, `Context`, the five panel-owned flags — the schema eye and gear, the
  connection switcher, the active-database selector, the Server Activity clock — and `DatePick`, the
  date field's calendar, which is a *grid of days* rather than a menu but is dismissed by every
  gesture that dismisses one), gathered by `MenuFlags::of(OverlayUi, SchemaUi, ConnUi, TabsUi,
  ActivityUi)` and closed by `close_except(keep)`.
  The calendar's membership is the whole reason the list is a list: it is opened from the row panel
  and from an open grid cell, it is closed by the workspace root, **and** it has to be closed by `GridState::dismiss_overlays`
  when a grid cell is clicked — a press the cell consumes, so the root never sees it. Reaching that
  through the shared list is what stopped it needing a second spelling in the grid. A trigger has to enforce this *itself*,
  because it absorbs its own pointer-down — it must, or the root's dismissal would close the menu the
  click is about to open — so the root handler never runs for it. **Both halves of that bargain are
  the trigger's**, and until recently two of the five click-opened menus had neither: the connection
  switcher (`lib::header`'s `switcher`) and the QUERY toolbar's active-database selector
  (`editor_pane`'s `db_selector`) registered only `on_click_stop`, which floem turns into an
  `EventListener::Click` handler and nothing else — so the root's `close_except(None)` ran first,
  closed the menu, and the `Click` behind it reopened it, and neither could be shut from the control
  that opened it. Both now carry
  `.on_event_stop(EventListener::PointerDown, widgets::menu_trigger_press)`, which is what makes the
  premise above true rather than nearly true. Absorbing the press is not free, though: it transfers
  the mutual-exclusivity duty from the root to the trigger, so each also calls
  `close_except(Some(MenuId::Connection))` / `close_except(Some(MenuId::ActiveDb))` in its own click
  handler — the shape the schema eye, the gear and the activity clock already had. Those three were
  the only ones making the call, and the two that weren't were exactly the two that weren't
  absorbing, which is why the gap stranded nothing on screen and nothing pointed at it. The
  selector's call sits **after** its "nothing offerable" early return, so an inert trigger closes
  nothing; its absorb is unconditional, because the root's dismissal is about the *other* menus and
  pressing a dead control should not close one elsewhere. `query_pane` carries a `menus: MenuFlags`
  on `QueryPaneParams` to make that call possible at all — `editor_pane` has no `Ui`. The three
  right-column panels take the flags the same way now: `activity_panel`, `snippet_panel` and
  `history_panel` each carry a `menus: MenuFlags` built at the call site in `lib.rs`'s right-panel
  branch, which is what let them stop taking `schema` and `tabs_ui` at all (`lib.rs`'s
  `whole_ui_gate`) — they reached those two only to build this.
  **`widgets::menu_trigger_gate::every_click_opened_menu_closes_the_others_itself` is what holds it**,
  a sibling of `popup_anchor_gate` and a source scan for the same reason: the thing under test is a
  set of call sites. It reads every `src/*.rs` in the crate with the test modules cut off and asserts
  each click-opened `MenuId` — `SchemaEye`, `SchemaGear`, `Connection`, `ActiveDb`, `ActivityClock`
  and `DatePick`, but not `Popup` or `Context`, which are opened on `SecondaryClick` where the root
  dismisses on the press and the opener runs on the release, one gesture — appears in a
  `close_except(Some(…))` somewhere, in any of the three spellings the crate uses for the path (`crate::widgets::MenuId::`,
  `widgets::MenuId::`, bare `MenuId::`). Deliberately
  weak: it counts `close_except(Some(` and `menu_trigger_press` registrations against the number of
  click-opened menus so a rename can't make it pass by finding nothing, but which site is which is
  not checkable from source. **`Popup` stays off the list even though several of its openers are
  click-opened** — the grid's toolbar dropdowns, `table_designer::suggest_chevron`, the cell
  editors' pickers — because that id names a *channel* many controls share, so one
  `close_except(Some(MenuId::Popup))` anywhere would satisfy a check meant to hold each of them.
  Each of those triggers still owes the call, and `widgets::open_picker` makes it for every picker; what is missing is a gate that can tell one from another. That list was written out three
  times in three files, and the third one added a flag the other two never learned about: opening the
  activity clock's interval dropdown and then clicking the schema tree's eye left **both** on screen.
  A stranded dropdown is not merely visible — its `focus_root` stays registered, and
  `innermost_focus_root()` being `Some` makes every newly opened query tab decline the keyboard.
  `close_except` is guarded per flag (`RwSignal::set` never dedups, and an unguarded write re-runs
  every style closure reading it), the workspace root passes `None` so a pointer-down keeps nothing,
  and `closing_leaves_exactly_the_one_menu_that_asked_to_stay` walks every id. Add a new dropdown by
  adding a variant and a field here, not a fourth copy of the list.
  **What actually holds a ninth variant to that list is `MenuFlags::slot`, not `MenuId::ALL`.**
  `slot(id) -> MenuSlot` is "what do open and close mean for this one", boxed so five
  `RwSignal<bool>` and three `RwSignal<Option<T>>` of three different `T` can sit in one array, and
  its `match` has **no wildcard arm** — so a ninth `MenuId` fails to compile until somebody says
  what opening and closing it mean. `ALL` is a hand-written literal like any other and is the
  second half; `every_variant_has_a_slot` is what asks for it. `close_except` was three `if`s and a
  five-element array before this, which a new variant compiles straight past. The tests read
  openness through `slot` for the same reason: the fixture's own hand-written list is exactly the
  shape that let `close_except` omit a flag, and the `open()` it replaced spelled all eight itself
  and left `Context` out of its loop, so "keep the context menu, close the rest" was asserted
  nowhere.
  **`MenuFlags::any_open` is the second reader of that list, and it is a refusal rather than a
  dismissal.** Nothing in the window's key handler closes menus — `close_except(None)` is on
  `PointerDown` — and a menu panel is a `focus_root` that returns `Continue` for every key but Tab,
  so a `Ctrl+…` reaches the window fallback with the menu still standing. Ctrl+W then disposes the
  tab under a grid menu whose entries are `Rc` closures over that grid's signals, and clicking one
  afterwards is a `get_untracked` on a disposed signal — a panic that takes every other tab's
  uncommitted edits with it; Ctrl+T opens a query tab whose autofocus declines, because the
  stranded panel's `focus_root` is still registered. It asks all eight channels and not just the
  two the grid uses, the gear, the eye, the switcher, the active-database menu and the activity
  clock stranding exactly the same way, and `any_open_sees_every_channel_by_itself` raises each
  alone — `closing_leaves_exactly_the_one_menu_that_asked_to_stay` opens all eight at once and so
  cannot tell seven-of-eight from eight-of-eight. It is deliberately **not** folded into
  `modals::modal_backdrop_up`, whose doc forbids menus in terms: that predicate also raises the
  modal layer's full-window box and the title-bar band, and a menu must do neither.
- **The panel owes an absorb too, and it is the same fact read from the other end.** The root closes
  every menu on any pointer-down, and floem delivers `Click` on the way *up*, only to a view that
  still exists — so a panel that does not stop its own `PointerDown` is torn down by the root on the
  press and the row's click lands on nothing. The menu opens and choosing an item does nothing at
  all, which is worse than the trigger bug above because the surface still looks alive.
  `on_click_stop(|_| {})` on a panel is not this: it stops a different event, arriving too late.
  Widening the root from five hand-written flags to `close_except(None)` put the last two panels on
  this hook, and they were the same two as before — `conn_menu_overlay`'s and
  `active_db_menu_overlay`'s, the only click-opened bodies that had never needed the absorb because
  the root had never closed them. Both now carry the bare `|_| {}` form, which is deliberate and not
  `menu_trigger_press`: a click on a menu row is a gesture within something the keyboard may still
  legitimately own, so `keyboard_nav` must survive it. **`widgets::menu_panel_gate::`
  `every_click_opened_menu_panel_absorbs_its_own_pointer_down` holds this half** — a third source
  gate, for the third time because the thing under test is a set of call sites. It slices
  `overlays.rs` at its column-0 `fn` headers and asserts each of the five click-opened menus'
  overlay bodies mentions `EventListener::PointerDown`; `popup_menu_overlay` and
  `context_menu_overlay` are out of scope for the reason they are out of `menu_trigger_gate`'s, and
  because their body is `menu_panel`, which carries the absorb once for both.
- **A trigger says it is open in the accent** — `widgets::menu_icon_color(open, hovered)`, one
  function for the schema panel's eye and gear, the Server Activity clock, and the results strip's
  copy, download and AI icons, each of which had spelled its own two-arm hover match. **Open
  outranks hover**, which is the ordering the whole rule turns on: the pointer is still on the icon
  it just clicked, so a hover that won would paint "this menu is open" and "you are about to open
  this menu" the same colour for as long as the menu is up. The trigger is the only part of an open
  dropdown in the user's eyeline — the panel drops *below* it — so a menu raised over a busy grid
  used to have nothing on screen naming what raised it. The grid's three ask
  `menu_anchored_at` **reactively** (`menu_is_mine_live`, a `.get()` twin of the `get_untracked`
  `menu_is_mine` the click path uses) so the style closure re-runs when the channel changes; the
  panels' four read their own `RwSignal<bool>`. The results strip's AI icon keeps its busy arm
  *first*: a request in flight makes the glyph inert, whatever the menu and the pointer are doing.
  **A trigger with nothing to offer is dim and stays dim**, through
  `widgets::menu_icon_color_gated(enabled, open, hovered)` — the results strip's copy and download
  icons on a 0-row result (*Data grid*), and its chart icon, which raises a modal rather than a
  menu and so passes `open` as `false`. It *composes* `menu_icon_color` rather than adding a fourth
  arm inside it, because five of that function's six callers are triggers that are always available
  and would all have had to start saying so. The gate outranks all three states and **hover is the
  arm that proves it**: an icon lighting up under the pointer promises a menu the click does not
  open, which is worse than no feedback at all —
  `widgets::menu_icon_tests::a_gated_trigger_stays_dim_whatever_the_pointer_does` pins that, and
  pins that the enabled path is still exactly `menu_icon_color`. The dim itself is
  `widgets::icon_disabled()` (`text_muted` at 30% alpha), **the one definition**, which
  `toolbar_icon` had inline and now calls: faded rather than hidden, since the icons in a toolbar
  are found by position and a control that vanishes when it is unavailable moves every icon beside
  it.
  The query pane's database selector follows the same order in its own colours — it rests in
  `bubble_claude_text` rather than `text_muted`, so it states the rule inline rather than calling
  the helper, and both halves (label and chevron) take the answer, the label by inheriting it from
  the stack (never by reading `db_hov` inside its `dyn_container`, which is disposed-signal
  territory — see the note there).
- **A trigger that toggles must also stop its own `PointerDown`**, and the two halves are not
  alternatives. The workspace root closes `popup_menu` on any pointer-down (`lib.rs`, the same
  handler that clears `keyboard_nav`), so a trigger that doesn't stop the press has its menu closed
  *before* the click arrives, and the click reopens it — down closes, up reopens, and the trigger
  never toggles however it decides. That was `suggest_chevron`'s bug, and it is a different one from
  the grid icons', which had the guard and no toggle: there the menu never closed at all. Guard
  without toggle re-opens what was never closed; toggle without guard closes what the click then
  reopens. The status-bar segments have carried both for as long as they have toggled; the
  connection switcher and the QUERY toolbar's database selector carried the toggle without the guard
  until they were given `menu_trigger_press` (above), and behaved exactly as this says — every press
  an open. Stop it with
  `widgets::menu_trigger_press` and not a bare `|_| {}` — swallowing the press also swallows the
  root's `keyboard_nav` clear, which that handler is there to repay (see *`widgets::keyboard_nav`*).
- **Not every menu can be *opened* from the keyboard**, which is a separate thing from navigating
  one. A menu on a ringed control can be — `suggest_chevron`, the Live Monitor's Export dropdown,
  and the grid toolbar's Copy / Save / AI dropdowns since the strip gained its ring — and the
  **schema tree** answers `Shift+F10` and the `ContextMenu` key on the row the nav cursor is on.
  The grid's cell and header menus, the editor's, the tab strip's and the connection list's still
  need a right-click.
  The tree's route is worth reading before copying it: focus lives on the tree **container**, never
  on a row, so a key arriving there knows neither which row it is about nor where that row is. Both
  ride on the per-row effect that already existed to scroll the cursor into view — `Nav::cursor_menu`
  is the row's own `CtxOpener` (the same closure its `on_secondary_click_stop` calls, so the two
  routes cannot offer different menus for one row) and `Nav::cursor_at` is where to open it.
  That point comes from **`on_move`**, which floem fires during *layout* with the view's window
  origin — not on pointer movement, and unlike `on_resize`'s rect, which is view-local and supplies
  only the height. It is the row's **content** corner, `origin.x + get_content_rect().x0`: a tree row
  spans the whole panel and the panel is flush left, so every row's own x is 0 at every depth and a
  menu anchored to the box hugged the window edge. The indent that makes the tree a tree is the row's
  `padding_left`. Both geometry signals are read *inside* the cursor guard, so only the cursor row
  subscribes and a scroll that moves it refreshes the point.
- **An open context menu marks the row it acts on** — a 1px rule above and below in
  `theme::row_menu_edge`, painted by `widgets::row_menu_mark(s, marked)` from the row's own `.style()`.
  The colour is `text_muted` at 45% — a fraction of the token rather than a darker one, so it
  recedes towards whatever is under it in either palette; `text_muted` at full strength still read
  as too light a rule (GitHub #22), as `text_dim` had before it. A menu is a panel floating clear of its row, and once the pointer is on the menu nothing on screen
  said which database, table or column the Drop was about. Two lists wear it. The **schema tree**
  (`Nav::menu_row`, `String` keys, through the one-line `schema_tree::menu_mark`) answered that
  first; **Manage Connections'** list (a local `menu_row: RwSignal<Option<u64>>`) needed it for the
  same reason and a sharper one — its menu's Duplicate and Delete carry the connection id
  themselves, and a right-click there deliberately does **not** select, because selecting runs
  `draft.load` and overwrites whatever is typed in the form. The helper therefore takes a plain
  `bool` rather than a row identity: that is what lets one definition serve two lists that identify a
  row differently, rather than the second one growing a lookalike that drifts. Neither mark is the
  selection or the nav cursor's highlight — a right-click moves neither (`resume_cursor` — a cursor
  that exists is the user's) — so this is a second, shorter-lived mark. The tree sets it in
  `marking_opener`, which wraps the row's `CtxOpener` where it is *built* rather than at the click,
  so the `Shift+F10` route marks too; the connections list has no keyboard menu route at all, so its
  `on_secondary_click_stop` is the only place that marks. Both clear through
  `widgets::clear_row_mark_on_close(menu, mark)`, an effect watching the menu channel go to `None`,
  which covers the closes a list's own code never sees — generic over both halves because the two
  keep the menu in **different channels** (the tree's `context_menu`, the connections list's
  `popup_menu`). Its guard, `mark.with_untracked(|k| k.is_some())` before the `set`, is not a
  micro-optimisation: `RwSignal::set` does not dedup, so an unguarded write re-notifies every row of
  a mark-less list on every close of any menu in the app.
  Two details are load-bearing. It is a **border, not an `outline`**: floem strokes a per-side border
  *inside* the view's rect (`paint_border`: top at y = 0.5, bottom at height − 0.5), so nothing bleeds
  onto the neighbouring rows and no `z_index` is needed to keep their hover backgrounds off it, while
  an `outline` — which floem inflates outward — would have needed one; and taffy sizes the **border
  box**, so `height(tree_row_h())` is unchanged and the rule costs 2px of content box, not a layout
  shift. That second half is why **the 2px is the caller's problem — only the caller knows where to
  take it from**. A connections row has no height at all: it is content plus
  `padding_vert(scaled(11.0))`, so a border grows it and shoves every list row below it down 2px for
  as long as the menu stands open, which is a list twitching under a right-click.
  `widgets::row_menu_mark_pad(base, marked)` returns one pixel less per side while marked, floored at
  0 — below 1px of base padding there is no pixel to give back, and a negative padding would take
  height *off* the row, the same shift inverted. The border is added in a style helper and the pixel
  is given back at the call site, so nothing but arithmetic says the two agree: `row_menu_mark_tests`
  pins them, and its load-bearing case is `the_rule_costs_a_padding_sized_row_no_height`, which
  asserts the *outer* box — padding ×2 plus border ×2 — is identical marked and unmarked, and was
  watched failing against the uncompensated version.
  The border-box half also holds for a row's **flex** children only, and the gap in it shipped a bug:
  taffy offsets a start-anchored *absolutely positioned* child from the border box plus the
  container's border on that side, so the 1px top rule moved `size_badge` — the tree's one absolute
  child — down by exactly 1px on right-click, and back when the menu closed, while the chevron, icon
  and name beside it never budged. The badge now centres on the cross axis
  (`align_self(AlignItems::Center)`), whose offset subtracts `content_box_inset` symmetrically and so
  cancels a rule that is on both edges; **any absolute child of a row that can wear this mark must
  centre rather than anchor to the edge the rule sits on**, and `size_badge` is the only one today.
  The key/index leaf is the only row outside the nav sequence, so it carries its own
  `key_row_menu_key` (a prefix that never reaches the persisted expansion set) and calls `menu_mark`
  itself — a marker that skipped the one row kind with a menu but no cursor is a marker the user
  learns not to trust. `every_row_key_family_owns_its_prefix` guards the shared key space.
- **A menu the keyboard opened gives focus back when it closes** — `widgets::set_menu_return`, set
  by the opener and **taken** by `menu_panel` as it builds, so the slot lives only between the two
  and a later menu cannot inherit a stale return. Folded into `close`, the path Escape and every
  action take. Gated on `keyboard_nav` because it is only wanted there: after a click, moving focus
  to the control clicked would take the arrow keys away from whatever had them (the grid's own cell
  navigation), and a click-away dismissal sets the channel to `None` directly and skips it anyway.
  Without it the surface that raised the menu goes **keyboard-dead**: the panel is a `focus_root`
  with no other root above it in the workspace, so its teardown drops focus and the next key reaches
  nothing. Both the grid toolbar's F6 and the tree's Shift+F10 hit exactly that.
  **The schema tree's *pointer* route needs the same return, and `keyboard_nav` is the wrong gate
  for it** — `schema_tree::arm_menu_return`, called from `marking_opener` so both of a row's ways
  into its menu arm it. A right-click left the tree with no focus once the menu went away: floem
  focuses a `keyboard_navigable` view on a **secondary** pointer-down exactly as on a primary one,
  so the click put focus on the tree and the panel's teardown took it away again — arrow keys, Enter
  and Shift+F10 all stopped answering and the nav highlight vanished with them, until something was
  clicked. The Shift+F10 route armed a return and said why; the pointer route armed nothing. The
  stated reason for withholding the slot after a click is that "moving focus to the control that was
  clicked would take the arrow keys away from whatever had them", and that premise does not hold
  here — floem has already moved focus, and the arrow keys were the tree's. So the guard is
  `Nav::focused`, which arms the return only when the tree actually held the keyboard and leaves
  every other trigger's rule intact. `Nav` carries the tree container's own `ViewId` for it
  (`tree_id`, published once the view exists), which was a local before there was a second caller.
  **The SQL editor's right-click menu arms one too, and unconditionally** — `set_menu_return(
  menu_return(…))` focusing the content view (`editor_view_id`), in the secondary-button branch on
  the line before `popup_menu.set`. Without it, closing that menu by Escape or by an entry left the
  keyboard on the grid: a right-click places no focus (`widgets::press_places_focus` — floem clears
  focus on every press and re-places it on a primary one only), so `hand_keyboard_back` fell through
  to the workspace's keyboard home, `refocus_grid`. Typing after Escape went nowhere, and *Rename
  alias* left a caret on every occurrence that no key could reach. The click-withholds-a-return
  premise fails here as it does in the tree: the right-click took the keyboard from nobody but
  floem, and the editor that raised the menu is where it belongs.
  `editor_menu_return_gate::the_right_click_menu_arms_a_return_before_it_opens` fails if that branch
  opens the menu with no return armed ahead of it.
  **The arming is unconditional; the restore is not — it stands down when the entry opened a
  modal**, and that is now `widgets::menu_return`'s rule for every menu (below), not a guard in the
  editor's closure. The restore is an `exec_after(ZERO)`, and so is `edit_field`'s autofocus, so
  when *Rename alias* opened its dialog both were queued in one pass and fired in floem's arbitrary
  timer order: when the restore won, the dialog's field never had the keyboard and what was typed
  into it — Backspace, `i`, Enter, seen in the window — edited the query behind the modal.
  **The stand-down is `menu_return`'s, and it asks whether a root newer than the return is
  mounted.** Every `focus_root` registers under a rising sequence number (`FOCUS_SEQ`);
  `menu_return` notes the last one handed out when the return is *built* (`focus_root_mark`), and
  the timer restores only if no root mounted when it lands is numbered past that mark
  (`widgets::restore_stands` over `newest_root_seq`, pinned by `restore_stands_tests`, which drive
  the registry rather than hand-picked values). A modal an entry opened is newer, so the return
  stands down whichever timer runs first; a menu opened *inside* a modal — the settings pickers' —
  closes with only that older modal up, so it still restores to the control it came from. **It
  compared ids once, and that was a regression**: it noted the innermost root's id at arming and
  restored only if the same id was innermost at landing. A root leaves the list only in its
  cleanup, which a `dyn_container` runs on a queued update, so a menu panel closed in the handler
  that armed a return was still innermost then. *Rename alias* / *Rename CTE* build their `back`
  inside the entry's action, before the menu closes, and a second right-click arms while the first
  panel is still registered — in both, the captured id was the since-closed panel's, nothing ever
  matched it, and the editor never got the keyboard back
  (`a_return_armed_under_a_since_closed_menu_restores`). The editor used to carry the check itself, as
  `innermost_focus_root().is_none()`, and kept it out of `menu_return` on purpose, because that
  question would have stranded the settings pickers. Kept local, it covered the one menu that had
  been seen to race: the schema tree's return had no guard, and it took the keyboard back from
  every modal its entries open — the Trigger editor, Edit table, Live monitor, ER diagram, Export,
  Import — leaving each deaf to Escape until it was clicked (GitHub #15). The grid toolbar's
  keyboard-only returns were exposed the same way. Asking for a root newer than the arming answers
  both cases, so the editor's guard is gone, subsumed.
  Standing down leaves the editor with nothing to come back to once the dialog closes — its root's
  own hand-back finds no other root — so the dialog carries its own return
  (`AliasRenameRequest::back`, under `alias_rename.rs`); without it Ctrl+Z after a rename did nothing
  until the editor was clicked. *Find usages*' list carries the same (`UsagesRequest::back`).
  **Opening that menu blurs the editor, and the caret must not go with it** — `looks_focused`
  (`focused || own_menu_open`), which `editor_style` reads for the caret colour and the
  current-line band in place of `editor_focused` alone. Both paint `Color::TRANSPARENT` while the
  editor is unfocused, on purpose: floem resets `cursor_info.hidden` to `false` on every cursor
  change, so the colour is the only handle, and it is what keeps the editor looking inert on load
  and while Ctrl+K, the schema panel or the terminal has the keyboard. But `menu_panel` is a
  `focus_root` that takes the keys for its arrows, Escape and Enter, so the right-click blurred the
  editor and the caret vanished with the menu still up — no sign left of where *Format selection*
  or *Rename alias* would act (GitHub #12). `own_menu_open` is set in the secondary-button branch,
  before `popup_menu.set` so no frame goes caretless, and cleared by an effect on the **next**
  change to `popup_menu` rather than on its going to `None`, because another menu replacing this
  one outright never passes through `None`. The right-click's own change is let through by
  `own_menu_opening`, a non-reactive `Rc<Cell<bool>>` the effect `replace`s with `false`. Focus
  routing is untouched — the menu still owns the keys, the return above is unchanged, and the
  diagnostic tip's retire effect still tracks `editor_focused` alone. `looks_focused_tests` pins the
  predicate (three cases); the effect lives in the view and has no unit test.
  **A right-click while that menu stands closes it before opening the next** — `if popup_menu
  .with_untracked(Option::is_some) { popup_menu.set(None); }` at the top of the secondary-button
  branch. `popup_menu_overlay` keeps the point a menu opened at (`opened_at`, a memo keyed on
  open-ness through `menu_open_point`) for as long as a menu is open, deliberately, so entries
  patched in place do not move it. So replacing the entries outright drew the new menu where the
  old one was, and from where the user looked the second right-click did nothing. Passing through
  `None` makes the overlay sample the pointer afresh, and it does so even though the reopen follows
  in the same handler because floem_reactive 0.2's `create_memo` is eager — it runs `f(None)` at
  creation and recomputes inside a `create_effect`, so the close is seen as it is set. **It comes
  first in the branch, ahead of `own_menu_open`/`own_menu_opening`, on purpose**: the effect above
  then takes the close as an ordinary change and clears any stale flag, rather than mistaking it for
  this menu's own opening. Only the editor's opener does this, because only the editor showed the
  fault: a second right-click in the history, the snippet library, the schema tree, the tabs,
  Server Activity and the connection list was checked by hand and already reopens at the new
  click. Why those differ was not traced, so a new opener should be checked the same way rather
  than assumed either way. Nothing pins the close: it decides through the overlay's memo and has
  no test.
  **Handing focus back is a focus *event*, and a handler that re-seeds state on one is a bug waiting
  for a caller.** The tree's `FocusGained` seeded the nav cursor from the open table unconditionally
  — correct while the only way in was a click from outside, and wrong the moment the menu started
  returning focus to a tree that was already focused and already had a cursor: closing the menu moved
  the highlight to whatever table happened to be open, with no keypress asking, and the next
  Shift+F10 or Enter acted on that row. The rule is `schema_tree::resume_cursor` — a live cursor is
  the user's, and only an unset one is seeded — which is what the handler's own comment always said.
  Shift+F10 also **re-resolves** `nav.selected` against `visible_nav_rows` before opening, exactly as
  the arrows and Enter do: `cursor_menu`/`cursor_at` are published by the cursor row's own effect and
  never cleared, so a row that has gone (collapse its parent, or refresh the database) left a
  callable opener and a stale window point — the menu for an invisible object, over an unrelated row.
  **An icon closing its own menu deliberately does not arm the slot.** The slot is consumed by the
  next `menu_panel` as it *builds*, and a toggle-shut builds no panel, so a return armed there would
  sit in the thread-local waiting for the next keyboard-opened menu anywhere in the app to collect
  it. `grid_toolbar`'s `close_mine` hands the keyboard back directly instead — `focus_icon`, by
  tabindex and deferred, because the strip may have been rebuilt by the action just run and floem's
  focus request has no existence check — and still only when `keyboard_nav` is true.
