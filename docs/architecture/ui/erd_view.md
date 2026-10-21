# `ui/erd_view.rs`

`erd_view.rs` — the **ER-diagram** canvas over `core::erd`. Edges are drawn by a custom paint
view (`EdgeCanvas`), *not* a Floem `svg` — `svg` doesn't repaint reliably on reactive change
here and blanked the edges on drag/hover. Zoom is **semantic, not a paint transform**: cards and
edges keep logical positions and multiply by `z` only at render, so text stays crisp at any
zoom. The surface is an infinite free pan (not a scroll view) — drag/middle-drag pans, Ctrl+wheel
zooms about the cursor, plain/Shift+wheel pans — and hit-testing maps cursor → logical space via
`(p − pan) / z`. A pan press captures the pointer (`request_active`) and its `PointerUp` ends
it; a middle-button release outside the canvas reached that listener only once vendored Floem's
fourth patch (*Floem 0.2 gotchas*) stopped gating non-primary releases on the view's rect —
before it, the pan outlived the button (GitHub #13). While `panning` is set the canvas's own
style wears `CursorStyle::Grabbing` (the fifth patch; GitHub #14): a captured view's moves are
routed straight to it and Floem takes the cursor from that view's style while the pointer is
inside its rect, so the cards under the pointer cannot override it — outside the canvas it
falls back to the default. The `apply_if` sits in the style closure *before* `.clip()`, like
the size beside it. No test covers it.
**The edge geometry is one memo with two readers** (`EdgeGeometry`, built by `edge_geometry`
over the positions, sizes and collapse maps). `rects` + `visible_map` + `edge_shapes` is
O(cards) + O(edges × 32) — a fresh `HashMap<String, Rect>` over every card, `card_metrics`
(O(columns)) per node, and a cubic bezier resampled into 33 points per edge — and it ran in the
canvas's `PointerMove` handler on **every raw pointer-move event over empty canvas**, the
gesture by which the user reaches everything, and again in `EdgeCanvas::paint`, so a move that
did change the hovered edge paid for it twice. The hover write is dedup-guarded, but the guard
sits *after* all of it, so it saved the paint and none of the work. This is the same cost class
as the position map's 8.4 ms per pointer move at 500 cards recorded under *Floem 0.2 gotchas*:
that handler was fixed and the sibling thirty lines below it cloned both maps and then did
strictly more than the original bug. The value is `Rc`-wrapped and compares by pointer, so it
never dedups — the point is to compute once per change of the inputs, not to suppress a repaint
the canvas's own effect already handles. The hit test reads the same memo through
`erd::nearest_polyline_of`, which is `nearest_polyline` over *borrowed* polylines and exists for
exactly this caller: rebuilding every edge's sampled curve to hand it a `Vec<Vec<Pt>>` was the
other half of the same per-pointer-move cost.
**The toolbar is responsive**, and the decision is `core::erd::fit_toolbar`'s while the
measurement is this module's: `chip_w` / `icon_button_w` / `zoom_unit_w` predict each group's
width *before* layout, from the same `TOOLBAR_*` constants the widgets are drawn with — the
`MENU_ROW_PAD` discipline, for the reason that one records. The toolbar reads its **own**
measured width via `on_resize` rather than re-deriving the panel's `ww * 0.8`; it is
`width_full`, so its width does not depend on the children being hidden and there is no
measure-hide-measure loop. The scope breadcrumb is the one variable-length item and so the one
that ellipsizes, and it does so only after every optional group has already gone.
`toolbar_metrics` hands the fit one cost per pill, and **which gap each cost carries is the
load-bearing part**: the first pill's includes `TOOLBAR_GAP`, the group's own leading gap, and
each later one `CHIP_GAP`, the gap before it. That is only true because `modal_frame` hides the
whole pill group once `fit().counts` is 0, not just each pill (`i >= fit().counts`) — an empty
`h_stack` is still a flex child and would keep the leading gap the first pill's cost claimed to
give back. `the_toolbar_pays_for_every_gap_it_draws` writes the per-pill sum out by hand.
A table carrying a `db_color` identity colour has its card **header** tinted with it: `header_bg`
composites the colour over `theme::erd_node_header` at `HEADER_TINT_ALPHA` (0.22) and is called
*inside* the header's style closure, so the themable half follows a live theme switch while the
identity hex arrives by value — the same split `db_color_dot` makes. **Each card's colour is
resolved once, untracked, before the cards are built**, and deliberately not inside that style
closure: it re-runs for every card on every pan and zoom, so a scan of the rule list there would
reintroduce the O(cards²) cost the `positions.with` borrow in `node_card` exists to avoid. The
lookup needs only the node id, because a diagram is scoped to one `(conn_id, database)` by its
`ErdTarget` and a node id *is* the display name `TableColorRule` is keyed by. The trade is that a
card doesn't repaint when a colour changes underneath it — a colour is settable only from the
schema tree's menu, which this modal covers, and reopening the diagram rebuilds every card. A
`NodeKind::Stub` card returns before both the header and the border and stays untinted, and the
*header* alpha is pinned by `contrast::tests::an_erd_header_tint_keeps_the_table_name_legible`,
which governs `HEADER_TINT_ALPHA` alone, since the header is the one surface where an identity
colour is a fill under text rather than a 6px dot.
The tint also carries around the **whole 1px border**, so a coloured card reads as one tinted
object rather than a tinted band in a neutral frame: `card_border` returns `theme::border()` for
a table with no colour, and otherwise the colour washed over `theme::erd_node_header()` by the
pure `tinted_border(tint, header, canvas)`. **The border's wash strength is its own, and is
picked by the canvas's measured luminance** — `border_tint_alpha` returns
`LIGHT_BORDER_TINT_ALPHA` (0.60) when `contrast::relative_luminance(canvas) > 0.5` and the
header's own 0.22 otherwise. That is the same reasoning as "ask a capability, never an engine",
applied to a palette: a future third theme gets sorted by the property that actually decides the
answer instead of by being recognised by name. Two strengths are needed because the Light
theme's header surface and canvas are nearly the same grey (`#EEF0F5` on `#EDEFF3`), so at the
header's 0.22 a tinted border came out *fainter than the plain `theme::border` it replaced* —
Amber at 1.09:1 against the canvas where the plain border manages 1.25:1. At 0.60 every preset
clears it, Amber still the floor at 1.29:1. It can't go much lower — Amber is a pale yellow
whose luminance sits near the canvas's, so even a full-strength rule only reaches ~1.5:1 there —
and raising it risks nothing, because unlike the header a border carries no text. On the dark
canvas 0.22 was already enough (1.67–2.16:1 against the plain border's 1.37:1, every preset
washing lighter than `#2E303A`), so dark keeps the header's strength and the border there lands
on exactly the header's colour, which is what makes the card read as one object. `tinted_border`
takes both surfaces as arguments rather than reading `theme` itself so the alpha choice can be
measured against every built-in theme and not just the loaded one:
`tests::a_tinted_border_is_never_fainter_than_the_plain_one` compares every `CONN_COLOR_PRESETS`
entry's tinted border against the plain border it replaces, in every `UiThemeKind`, and
`tests::the_border_alpha_follows_the_canvas_not_the_theme_name` pins both branches (both
strengths reachable from the two built-ins; white → light, near-black → the header's). The first
is a **comparison, not a WCAG floor**, which is why it lives here rather than in `contrast`'s
pairing table — borders are furniture, and holding one to a text floor would mean nothing.
**Ctrl+F opens a find bar over the canvas** (`Find` + `find_bar`, over `erd::search`), carrying
the editor's and grid's chrome in the diagram's top-right corner. It is a **sibling of the
canvas layer, not a child**: the canvas is `.clip()`ped and pan/zoom-transformed, so a popup
inside it would scroll away with the diagram. The flex-grow wrapper that measures the viewport
is therefore a `stack((canvas_inner, find_bar(…)))`, and because that wrapper *is* the modal
body, `inset_top(10.0)` already means "10px below the toolbar" without knowing how tall the
toolbar is. There is **no prev/next pair**, unlike the other two find bars: those step a caret
through an ordered document, while this lights up every match at once and moves the canvas only
when `erd::sole_node` names a single card — a "next" button would have to invent a sequence over
a 2-D canvas before it had anything to do. The bar also carries `on_event_stop(PointerDown)`,
because the canvas pans on a primary drag and the popup sits on top of it, so aiming for the
text field would otherwise drag the whole diagram out from under the pointer. That `Stop` has a
cost the handler now pays: it is returned from inside floem's children loop, so no ancestor
reaches the default block that would have re-focused the modal root, and floem takes focus on
every `PointerDown` before dispatch — a press on the bar's padding therefore left
`app_state.focus` at `None` and the diagram answering no keys. So the handler hands the keyboard
to the innermost focus root when the press **missed the field**, and still does nothing when it
landed on the field itself, which is what its own comment was protecting.
**That test is a frame, not a half-plane**, and it is the pure `press_missed_the_field(pos,
field, pad)` rather than an expression in the closure, because the closure cannot be tested and
this is the whole of the decision in it. The bar pads on all four sides, so the region that is on
the bar but not on the field surrounds it; the handler asked only `pos.x > pad + width`, which is
right for the count readout and the ✕ and wrong for the other three sides. A press on the 6 px
strip above the box, or the 8 px to its left — where a user aiming at the box and being 3 px off
lands — was a miss the bar consumed while handing the keyboard to nobody, and Escape and Ctrl+F
stayed dead until the canvas happened to be clicked.
`a_press_misses_the_field_on_every_side_of_it_not_only_the_right` walks all four,
`the_field_owns_its_own_top_left_corner_and_not_the_pixel_past_it` pins the half-open ends, and
`the_band_follows_the_interface_scale` holds the older half of this. **Both terms are scaled**,
and there the metric rule is behaviour rather than pixels: the band is `[pad, pad + size)` on
each axis against a bar whose padding is `theme::scaled(8.0)`/`theme::scaled(6.0)` and a field
styled `width(theme::scaled(190.0))`, while the test was once written `8.0` and `190.0`. It
was right at 100% and wrong at every other interface scale — at 160% a press in the rightmost
4.8 px *inside* the box was judged a miss and handed the keyboard away, undoing the focus the
click had just set, and at 80% the mirror hole treated a press 1.6 px *past* the field as on it,
so the keyboard was never handed back and the diagram went keyboard-dead: the exact state this
handler exists to prevent. The size is read from the laid-out view and only falls back on the
constant, so a change to one cannot leave the other describing a field that moved.
`Find::dismiss` does the same, covering Escape-in-field and the ✕ together: closing the bar
removes the focused editor, and floem clears focus *silently* when a focused view is removed —
after which Escape didn't close the diagram and Ctrl+F didn't reopen the bar until the user
clicked something.
One `Memo<erd::Matches>` per diagram recomputes per keystroke and each card derives its own
`Option<NodeMatch>` memo from it, so a card whose match didn't change doesn't re-render because
a character was typed elsewhere — memos specifically, since `dyn_container` is built on
`create_updater` and does not diff, it rebuilds whenever a dependency fires.
**`Matches` is indexed by node** for that per-card question: scanning the hit list made it
O(cards × matches), and a one-character term matches every card — 0.65 ms of pure comparison at
500 cards, before floem does anything. The same reasoning keeps the matched **columns** out of the
row stack's rebuild key: they travel into `column_row` as a per-column `Memo<bool>` read inside
the row's own style closure, because the highlight is a colour and an outline. Carried in the
`dyn_container` selector instead, a broad term rebuilt ~10,000 views per keystroke on a 500-card ×
20-column diagram — a memo per row costs a cached read on a pan frame, where a scan there would
have reintroduced the O(cards²) trap this module already documents twice. The pan and the flash
are **guarded writes** for the same reason: every card's style closure reads both, and `set` never
dedups, so typing another character that resolved to the same card was two more full restyle
passes over a canvas that had not moved. Highlighting is
`theme::match_highlight` on the table name and on the matched column names, the same colour the
schema tree marks a filter hit with, and deliberately **not** the tree's per-character
`highlight_text`: that bakes a fixed font size into a text layout, and a card's type scales with
the zoom, so the name would stop growing with the diagram. A find hit outranks a column's key
tint — the gold/purple says what the column *is*, which is still true and still on the glyph
beside it.
**A hit is marked twice, and only one of the two marks survives a tinted header.** `name_paint`
is that decision: over the plain header the recolour measures 5.11:1 (Dark) / 5.69:1 (Light),
while over an identity colour washed at `HEADER_TINT_ALPHA` it measures 3.11–4.04:1 on Dark and
as low as 4.38:1 on Light — under the 4.5 floor the pairing table sets for this exact site, worst
of all on the card the search has just panned to and ringed. So a tinted header keeps
`theme::text()`, which *is* gated there, and wears its **bottom border** in the match colour
instead: a border carries no text-legibility debt, moves nothing, and is the language the matched
rows already use one weight down. It is a function so
`an_erd_header_tint_keeps_the_table_name_legible` can ask what the code paints instead of
restating a foreground the code has since stopped using — which is exactly how 3.11:1 shipped
past a test written for this surface one commit earlier. The three plain `match_highlight`
paintings are ordinary `PAIRINGS` rows.
**A stub card is a card that can be found**, so the per-card match memos are derived *above* the
stub branch: `erd::search` matches a stub on its name deliberately, `sole_node` returns it and the
canvas pans to it, and the early return used to skip both the recolour and the flash ring — a
search that said "1 match" and marked nothing.
**Its title carries `min_width(0)` + `text_ellipsis()` like the real header's**, and without
them the name painted straight past the card's own border onto the canvas. `min_width(0)` is the
load-bearing half — taffy's automatic minimum for a flex item is its content size, so nothing
shrinks the text node without it — and a stub is the worst case by construction: `stub_width`
clamps the card at `NODE_MAX_W`, and a stub's id is the one label that is *always*
database-qualified (`analytics_warehouse.customer_order_line_items`), so it is the longest thing
any diagram holds and the clamp is routinely reached. It is a correctness fix rather than a
tidy: `export_scene` ellipsizes this same name against the same 320 px, so the saved picture
read `…customer_order_line_i…` where the screen showed the whole thing overflowing its box —
precisely the canvas/export drift that path exists to prevent. The truncation **tooltip** the
real header gets is deliberately not copied: that one hangs on the header *row inside* a
draggable card, while a stub has no inner row, so it would have to go on the whole card — making
a 340 px box swallow drags to reveal a name, which is a trade filed rather than taken.
The pan is a `create_effect` on the matches memo: given a `sole_node` it calls `erd::center_pan`,
which solves the cards' own `pan + logical·z` for `pan` — pure and tested beside `fit_bounds`,
the whole-diagram case of the same arithmetic, because a sign slip or a `w` where an `h` belongs
leaves the sole match off screen while every readout still agrees a match was found. It reads
viewport, positions, sizes
and zoom **untracked** so panning can't re-trigger it, then flashes the card for `FIND_FLASH`
(3s). **The flash ring is an `outline(2.0)`, not a fatter border** — a border is part of the
box, so widening one would nudge the card's content by a pixel as the ring came and went — and
it is read with `with`, not `get`, so it doesn't clone a `String` per card on every pan and zoom
frame. `flash_seq` is bumped per flash so an expiring timer only clears the outline **it** set
(searching twice inside three seconds otherwise lets the first search's timer wipe the second's
ring early), and the expiry reads it through `try_get_untracked`, because the modal can close
inside those three seconds and take the signals' scope with it.
**The modal's whole keyboard policy is one `on_event(EventListener::KeyDown, …)` on the root**,
where a single `on_key_down(Escape, …)` used to be, so the order Escape is offered around in is
readable in one place — the grid's `grid_key` for the same reason. It sits on the same `ViewId`
as `focus_root_with_ring`'s own Tab handler and does not displace it: `add_event_listener`
appends, and `on_key_down` was only ever `on_event(KeyDown, …)` with a key filter in front.
Escape dismisses the find popup first and closes the diagram only when there was no popup to
close: `Find::dismiss` returns whether it *was* open, and that answer is the whole mechanism.
Ctrl+F opens it. The field's own `FieldCfg::on_escape` dismisses too and covers the case where
the field has focus, since floem registers the editor's KeyDown listener with `on_event_stop`
and it consumes the key outright; the root handler covers the rest of the modal, which is most
of it, the canvas being pointer-driven — pan, zoom or drag anything and focus has left the
search box. That mirrors the grid's focus-independent Escape rather than trusting the field
alone. `modal_frame` takes a `find: Option<Find>` and the two message-only bodies pass `None`:
nothing to search there, so neither binds Ctrl+F.
**Export is the toolbar's fourth control**, a `menu_button` raising the app's shared popup
channel — `overlay.popup_menu`, the only surface painted *above* this modal (it is last in the
workspace stack for exactly that reason). The menu saves any of the six `ErdExportFormat`s and
copies the five text ones from a `Copy as` submenu; pressing the icon again closes its own menu,
off the same `PopupAnchor::BelowIcon` equality the grid's dropdowns use. This menu is what
turned up the hoisted-submenu bug (*Popup menus*): anchored at the right end of a toolbar, its
submenu flips left on any window narrow enough, and a flipped submenu used to be unclickable.
The anchor is the **button's** rect, taken from `on_move` + `on_resize` rather than by adding
the control's padding back onto the glyph's origin: a menu hung off the 16px glyph's bottom edge
rides up into the button that opened it. `export_menu_w` is `pub(crate)` because the *Chart*
modal's save menu opens at this same width (`chart_view.rs`).
`export_scene` is the bridge to `core::erd_export`: it reads the same four signals the cards
render from (`positions`, `sizes`, `collapsed`, plus the resolved tint) and reuses `rects`,
`visible_map` and `edge_shapes` — **the very functions `EdgeCanvas::paint` calls** — so a
dragged, collapsed, colour-tagged diagram exports as itself and cannot drift from the canvas.
The custom paint view was never the obstacle the TODO thought it was: `edge_shapes` already
returns a flattened polyline and marker segments, and both drop straight into SVG elements.
Text is ellipsized here rather than in the core, with `measure_text_px_at`/`_bold_at` — the
same measurer that sized the cards. The "+N more" note is carried into the export but "show
less" is not: the first says the card is showing part of a table, which stays true wherever the
picture ends up, while the second is an instruction to a canvas that isn't there.
A column *type* is measured **once per distinct string**, not once per row (the `type_w` map):
each `measure_text_px_at` builds a fresh cosmic-text layout, and a type is the most repeated
string in a schema — a database is mostly `int`, `varchar(255)` and `datetime` — so per row that
was one layout for every row of every card, keyed by the string it is a few dozen for the whole
diagram. The column *names* are deliberately not cached: a hit there would be the exception.
The capture happens **before** the save dialog opens, not in its callback (`ErdDoc`): the dialog
is modal and these signals belong to the modal behind it, so a callback that came back for them
could be reading a disposed scope — and it also means the file is a picture of what the user
was looking at when they chose the format, the rule the grid's row snapshot follows. **What is
captured is as little as the UI thread is obliged to do.** `ErdDoc` is `Text(String)` for the
four text formats and `Scene(Box<SvgScene>, Option<f32>)` for a picture: `export_scene` must run
here because it measures through floem's font system, but `to_svg` is pure, so building the
document, rasterising a PNG and writing the file are all the worker's (`ErdDoc::into_bytes`,
called inside the app's `export_erd` `spawn_blocking`). Measured at 500 tables × 20 columns, that
took the UI-thread stall between the click and the save dialog from 65 ms to 25.5 ms.
A third variant, `Raster(String, f32)`, is a **finished** SVG to rasterise at that scale — the
*Chart* modal's PNG (`chart_view.rs`), whose document is bounded — at most 2,000 marks or 50
bins — and so is built on the UI thread, and whose raster is still the worker's: `into_bytes` hands it to `erd_raster::png_from_svg`,
and `into_text` answers `None` for it as for a diagram's PNG. That modal saves through this
same `export_erd`, so the writer is no longer the diagram's alone.
`ErdDoc::into_text` is the exception and says why: the clipboard is synchronous, so *Copy as SVG*
is the one caller with nowhere to hand the work and pays for the document here — and it answers
`None` for a PNG, which that channel cannot hold, which is why PNG is not in the copy menu.
**That copy's clipboard write is checked**, where it was `let _ = …` followed unconditionally by
the green confirmation — so a host whose clipboard provider never initialised
(`ClipboardError::NotAvailable`) or whose backend refused the write (`ProviderError`) was told
"Copied as Mermaid" with nothing on the clipboard. The file half of this same menu already
routed its failure through the notice bar; the channel was there and this caller declined to use
it. The two arms are spelled out rather than formatted, `ClipboardError` having no `Display` and
a `Debug` that is not a sentence: `NotAvailable` is about the host and says so, while
`ProviderError` carries the platform's own words and is worth repeating. The
outcome lands in the diagram's own `notice_bar` at the canvas's bottom edge rather than the
app's shared error modal, which is painted *under* this one. It arrives as the `ExportOutcome`
the results export shares, of which a diagram uses two: one document has no row count and
nothing to cancel, so `export_erd` reports `Done(0)` and never `Cancelled`. The save dialog and
that handler are one function, `save_document(doc, dialog_title, default_name, fmt, export,
say)` — `pub(crate)` because the *Chart* modal's save calls it too, titled *"Save chart"* where
the diagram's `save` passes *"Export diagram"*, the title a `&str` so the chart's translated
one passes — so the two modals read an outcome by one
decision: `Done` says *"Saved {name}"*, `Cancelled` nothing, `Failed` its message as a failure.
The handler still **spells all three arms out**, the way `monitor_view::save_log` spells its own and for the
reason that call site gives — the arm is unreachable today, but the rasterise it wraps is one
change away from being cancellable, and the catch-all `_ =>` this had would then report a
cancelled export as "Saved <name>". A confirmation fades after
`NOTICE_LINGER`; a failure stays until dismissed, since it is the only place the reason is
written down. The fade is `flash_seq`-guarded and `try_get_untracked`-read for the same two
reasons the find flash is.
**Its words are the catalogue's** (`erd-*`) — the title
(`widgets::modal_title_borderless` takes `impl Into<String>`, so its literal callers still
compile), *Scope* (which `toolbar_metrics` measures through the same message, so the label and
the width budgeted for it cannot disagree), the three count chips (`chips`: `erd-chip-tables`,
`erd-chip-relationships`, `erd-chip-hidden`), the find bar's placeholder (through
`FieldCfg::placeholder_of`), a collapsed card's `+N more` and *show less* — `erd-more-columns`
is also the export model's `more`, so a saved picture says what the canvas says — the
not-loaded and no-relationships notes, the export tooltip (`menu_button`'s tip is a `String`),
*Copy as*, the export dialog's window title, and the notices: nothing to export or copy,
`erd-copied-as` (`$format`), `erd-clipboard-unavailable`, `erd-copy-failed` (`$error`) and
`save_document`'s `erd-saved` (`$name`, falling back on `erd-unnamed-file`). **The chips stay
unpluralised in English** — `{ $count } tables` reads "1 tables", exactly as the `format!` it
replaced did, because each phase leaves English byte-identical; German selects a plural on
`$count` for the tables and relationships chips (its hidden chip's phrase does not change with
the count), and English can take one when the plural phase comes.
`the_counts_and_notices_read_as_they_did_in_english` pins the English, `1 tables` included; the
export-model test's `"+2 more"` passes unchanged. English stays: the formats' names
(`ErdExportFormat::label`), the zoom percentage, a not-loaded schema's `—` scope, the
`#rrggbb` colours `hex` writes, and `diagrams.json`.
