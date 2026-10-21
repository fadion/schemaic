# `ui/widgets.rs`

`widgets.rs` — reusable widgets: `menu_panel`/`MenuEntry`, `keycap`/`keycap_width` (the one
keycap, drawn by the palette and the menus alike), `modal_title`/`panel_style`/
`menu_item_style`, `window_size`, `autohide`/`autohide_tracking`/`shift_hscroll`/`wheel_hscroll`
scroll wrappers, `wheel_pan` — the one reading of a wheel event as a pan, Shift meaning
horizontal only and taking `y` when `x` is 0 (Windows sends Shift+wheel as vertical), which
`shift_hscroll` and every grid pane that forwards its wheel share (`wheel_pan_tests`),
`check_box` — **the app's one checkbox**, drawn by every multi-select list there is (the import
review list and the dump modal's table picker), so a picker cannot quietly grow a second look;
it fills and empties **by style, never a rebuild**, which is why its `checked` predicate is
`Clone` (the fill and the tick each need their own copy inside their own reactive closure); and
`link_button`, the "Select all" (`accent`) / "None" (`text_muted`) pair that sits above such a
list — links rather than footer buttons, because they adjust the list the user is reading
instead of answering the modal, and `in_ring_button`-wrapped because the only other way to
change a selection is clicking every row, which is not a thing a keyboard can do —
`section_title` — a panel's title, and **the part of its header that gives way**. The headers
are `h_stack((section_title, buttons))`, and a title without `min_width(0)` kept its text's
width, so a narrowing panel pushed its buttons out until they clipped (found under the doubled
pseudo-locale, every label said twice, since removed). It now carries `min_width(0)`/`flex_shrink(1)`/
`text_ellipsis`, and `toolbar_icon`'s container `flex_shrink(0)` as well as its glyph, so the
title ends in a `…` and the buttons never give up a pixel — the rule `modal_footer_split` and
the results strip follow, a control clipped in half not being a control —
`centered_msg`/`toggle_icon` (whose `enabled` is **not optional**: every panel
toggle in the footer needs it, so the ungated shims that passed `|| true` were only hiding the
fact — see *The footer's panel toggles* below), `tip_when`, `measure_text_px` (and
`measure_field_text_px`, text as an `edit_field` draws it — in `MONO_FAMILY` when the field is
`mono` — which is what places a suggestion ghost after the typed text: the column-type field is
mono, and a body-face measurement lands the tail inside its own text),
`jump_to_bottom_button`. Also `dedup_key` — the one-line `create_memo` wrapper a
`dyn_container` key goes through so a write that doesn't change *this* node's answer doesn't
rebuild it. It is here rather than in either caller because two of them have the same shape, a
shared map keyed by node: the schema tree's `expanded` and the ER diagram's `collapsed`. It was
`schema_tree`'s until the second one needed it; the measurements and the two pins are under
*Floem 0.2 gotchas*. Also `Clearable`/`clear_if_any`/`retain_if_any`/`retain_pairs_if_any` —
the never-write-a-no-op guards, here because they were module-private to `grid.rs` and so
unreachable from `schemaic-app`, which held the only two live violations of the rule they
enforce. `Clearable`/`clear_if_any`/`retain_if_any` are re-exported from `lib.rs` for that
crate; `retain_pairs_if_any` is not, its only callers being `grid.rs`'s three `dirty` filters
(*Floem 0.2 gotchas*, `update` doesn't dedup).
Also `sparkle_action` — the sparkle-plus-label "AI fix" the editor's
error bar and the error modal both offer, in one definition because the two had already drifted
to different colours by the time there were two of them; **neither half sets a colour**, so the
row's own tints the SVG's `currentColor` and the words together and one `hover` covers the pair
(set per child, as the first copy did, the icon stays dark while the label lights up).
**Its `ring` is `Option<(FocusRing, u32)>` because this is the one button family with a caller on
each side of that line** — the error *modal* has a ring, the editor's error *bar* sits in the
workspace and joins floem's own traversal — and neither was answered before: the row was a bare
`h_stack` with an `on_click_stop`, and floem's `can_focus` requires membership in
`keyboard_navigable`, so its own `view_tab_navigation` skipped it too and the error modal's
**only** two actions, AI fix and Explain, could not be pressed by any key. `Some` goes through
`in_ring_button` (the caller's face carries the click, the wrapper carries the keyboard and the
outline) and `None` through `key_pressable`, which owns both listeners itself; it returns
`AnyView` for that reason. That fix is two halves and needed both:
`focus_root_gate::every_modal_root_publishes_a_ring` finds a modal root with no ring — the error
modal was one — while `key_pressable_gate::a_sparkle_action_can_be_pressed_from_the_keyboard`
holds this function to one of the two helpers, because giving that modal a ring with nothing
registered in it hands it an **empty** one and `ring_step(0, …)` returns `None`. Also
`MenuId`/`MenuFlags` —
the single list of the app's mutually-exclusive dropdowns, which every trigger closes the others
through (*Popup menus*), and `row_menu_mark`/`row_menu_mark_pad`/`clear_row_mark_on_close` — the
rule a row wears while its own context menu is open, which lives here rather than in either list
because the schema tree and Manage Connections must wear the *same* mark and neither owns the
other (*Popup menus* again, for the 2px the caller has to give back).
Also `room_below` — the tallest a dropdown hung under an anchor *by hand* may be, for the three
in `overlays.rs` that are not `menu_panel`s (*Popup menus*): the window under the anchor less
`scaled(8)`, floored at `scaled(80)` so an anchor near the bottom edge still leaves a few rows,
that floor itself clamped to the window, and `0` — read as "no cap" — before the window has
been measured, the answer `menu_panel`'s own `menu_max_h` gives. Callers read it inside the
panel's style closure, so a resize moves the cap with it. The arithmetic is `cap_below`, pure
and pinned by `cap_below_tests` (four, run red against a stub returning `0.0` first).
Also `focus_root` — the **one** way an overlay claims the keyboard (see the Floem gotcha on
directed key dispatch): it takes focus on build *and* registers the view so Escape in a
text field inside it hands focus back rather than dead-ending. Never spell out
`.keyboard_navigable().request_focus(|| {})` again; that pair is what left every modal
unclosable from the keyboard while a field was focused.
Also the **shared modal form chrome** every modal wears — `form_setting`/`form_section`/
`form_separator`/`form_gap()`/`control_button`/`modal_footer`. Manage
Connections set that shape and Import followed it; a new modal builds on these rather
than copying them a third time. `form_setting` and `form_section` take `impl Into<String>` and
hand it to their `_owned` siblings, so a `tr!` label passes as readily as a literal. `field_button` is `control_button` with no vertical padding,
for `FieldCfg::button` (under `lib.rs`), which stretches it to the field's height; both go
through a private `control_button_padded`. Every control button centres its label both ways
(`items_center().justify_center()`) — nothing at its natural size, and what keeps the label
mid-box once a field has stretched it.
**A long form's sections are `form_section_header` over its group, `form_section_gap()`
(`scaled(36)`) apart, and no rule between them** — Manage Connections, today; Settings was the
other until each of its sections became a page of its own (GitHub #16). The gap is wider than any
gap inside a section (16 in Settings, 20 in the connection form), which is the whole of what
sets the sections apart. `form_section_header` is `form_section` in `theme::form_heading`
(the palette's `accent_hover` — the lighter blue on dark, the deeper one on light) with a
`margin_bottom(scaled(2))` added, built on it rather than restyled beside it — on
`form_section_owned`, strictly, since it takes `impl Into<String>` so a translated heading
(`tr!`, an owned `String`) passes as readily as a literal. Import and
Export (`import_view.rs`, `dump_view.rs`) head their sections with it too, at their own
spacing; everywhere else keeps `form_section` in `text` — the object editors, the DDL preview,
Run script and the fact panels head a few lines each, where a heading is a label rather than
a divider, and the blue was judged to be noise there. A point more size
and a rule under each heading were tried first and dropped: a long form ended up striped
with lines, and the colour sets a heading off with nothing added. Settings' version caption
used to copy the margin, so the air under it matched the air under `General`; it moved to the
foot of the section list and copies nothing now (under `settings.rs`).
**Every hint under a control is `form_hint_style`** — `text_faint` at `font_hint`, `width_full`
— through `form_hint` for a literal, `form_hint_label` for text that follows a signal, or the
style itself where a hint needs one thing more (the init-SQL line's `error` when the text is
refused). The Settings AI page and the connection form had hand-rolled theirs in `text_muted`
or `text_dim`, and the user saw hints a shade apart on the same page; `text_faint` is the colour
they chose for all of them. `text_muted` at 60% was not an option: it composites to 1.70:1,
under the `Recessive` floor of 2.0. `width_full` is in the style because a text sizes to its
content otherwise, and a long hint runs off the form instead of wrapping — the Jump hosts hint,
inside a `dyn_container`, did.
**`pill_tab_style(s, on)` is a modal's section-tab look**, lifted out of
`table_designer::tab_strip`: body size, `scaled(12)`/`scaled(7)` padding, radius 6, filled in
`pill_active_bg`/`pill_active_text` when `on`, `text_dim` with a `pill_hover_bg` hover when not.
The designer's *Table | Columns | Indexes …* strip and Server Variables' *Variables | Status*
switch both call it, so the two cannot drift apart again — the switch wore Server Activity's
underlined labels until the user asked for it to match the designer's tabs. **`count_pill`** is
the third caller — the filter pill with a count, shared by Compare results' *All · Changed ·
Added · Removed* and the Live monitor's *All · Inserts · Updates · Deletes*: a coloured dot
naming the kind (none for *All*), the name, the count in `text_faint` while inactive, wrapped in
`in_ring_button` at radius 6 so the focus outline follows its corners, Enter or Space picking.
On the active pill the dot takes `pill_active_text`, because Compare's *Changed* blue vanished
into the pill's fill. Beside it, the fixed-row tables' cell faces: `cell_text` (one line in a
`fn() -> Color`, ending in `…`), `struck_text` — Floem 0.2's text has no line-through, so it lays
a 1px absolute rule over the text at `inset_top_pct(52)` — and `transition_text`, `old → new`
with the old struck, each side shrinking to its `…` before the arrow does. Server Activity's
own *Sessions | Locks* switch does not use it either: it is a switch between views of one
panel, not a modal's sections, and takes `segmented_tabs` below.
**`segmented_tabs(tabs, current)` is a tab bar** — the tabs in equal shares of one bar
(`flex_grow(1)`, basis 0), labels centred at body size, a click setting the `RwSignal<T>`. The
bar wears a text field's surface and outline (`bg_chrome`, a 1px `field_border`, radius 6,
`scaled(3)` padding and gap), so it reads as a control of the same family as the search box it
sits above. The active tab is filled in `btn_primary` (radius 4) with a `text` label and answers
no hover — it is already where a click would go; the others are `text_dim` and a hover only
brightens them to `text`. Server Activity's *Sessions | Locks* was its first caller, replacing the
row of accent-underlined labels it drew before, and Query History's *Current | All* the second,
replacing an unlabelled icon toggle; `pill_tab_style`'s loose pills stay the look of
a modal's section strip. A label is any `L: Into<String>` rather than the `&'static str` it
was, so Query History's `tr!` labels pass as readily as Server Activity's literals.
**There is no `footer_button` any more.** Every modal with a footer bar wears the filled
`action_button`, and the text-only button that sat beside it at a smaller padding left with the
last of those footers. The rest of that family went with GitHub #25: `dialog_button` (over a
private `text_button`), whose only callers were the two question dialogs — the transaction
prompt and the confirm modal — and which left when they were given the standard modal (under
`overlays.rs`). It had begun as a private copy in each of the two, and its `ring` was required,
like `modal_title`'s, because the transaction prompt's buttons had once sat outside every ring
while Escape there was deliberately dead — so no key did anything at all and the user had to
reach for the mouse to answer a question about their own uncommitted writes. `action_button`
requires its ring too, so that holds by construction now. (Escape answers *Cancel* there —
GitHub #15, under *A modal's click-to-dismiss* in *Floem 0.2 gotchas*.) A new footer, or a new
question, takes `action_button`; it does not revive either of the smaller ones.
**`modal_footer_split` shrinks the status and never the actions.** A flex item's `min-width` is
`auto`, so a long sentence in the left slot refuses to compress and pushes what follows it out
of the panel — which is how the binary panel's load line shipped its three buttons off the
right-hand edge of the modal, and what a long enough "Saved to …" path would have done to the
same footer on any other day. The status is wrapped in a container with
`min_width(0)`/`flex_shrink(1)` and the actions in one with `flex_shrink(0)`. Every modal shares
this row, so the fix belongs to the widget rather than to the one sentence that exposed it, and
the caller's half is `text_ellipsis` on a status that can run long — so it ends in a `…` rather
than in a clipped word. **`min_width(0)` goes on the status itself as well as on that wrapper**,
because `min-width: auto` applies to *every* flex item: relaxing it on the container left the
status inside with taffy's automatic minimum, so it never compressed, `text_ellipsis` never had a
narrower box to end a line in, and the documented contract could not be kept by a caller who did
everything right. Eight of the seventeen call sites had noticed and were spelling
`status.style(|s| s.min_width(0.0))` themselves; the other nine were not — a helper whose
contract only holds if the caller repeats half of it is one that will keep being called wrong.
**Every status these footers carry now ends in a `…` rather than wrapping or clipping** — an
audit under text twice English's length found the rest still overflowing. Only Chart's and Result diff's
export notices, the first validation problem of the object, view, trigger, database and
account editors and the table designer, and the designer's index hint go through
`footer_error` or `ellipsized_text` (both below), so only those can be read whole on hover —
Export's and Run script's outcomes left this row for `modal_footer_noted`. The rest are a bare
`text`/`label` styled `min_width(0)` and `text_ellipsis`, and carry **no tooltip** — a cut
line there is cut for good: Users' and Server Variables' counts, Compare's selection note,
Export's and Run script's progress lines, Result diff's `summary_line` (which also lacked the
`min_width(0)` above), the *N changes* lines of the object, event, routine, view and trigger
editors and the table designer, and the event and routine editors' *reading source* lines.
**`modal_footer_noted` is for a sentence too long to end in a `…` beside the buttons**: the note
gets a full-width row of its own above them, wrapping at the modal's width, with the actions
right-aligned below and never shrinking. **Never spell that as a `v_stack` handed to
`modal_footer`** — its actions slot is `flex_shrink(0)` and as wide as its content, so a
`width_full` inside it has no definite width to be a percentage of, and the sentence laid out on
one line at its natural length: Import Connections' "Added N connections." line took Close and
Import past the modal's right edge exactly that way. Here the note is a direct child of the bar,
whose width is the modal's. A caller whose note is sometimes absent hides it with
`Display::None` rather than leaving it empty, or the empty row still takes the column's gap.
**Export's and Run script's outcomes are its other callers**: their done, stopped and failed
notes went through the one-line footer once, and that made the point of each line hover-only
— the tables a dump did not find, the objects it left out, the `.part` path, a script's count
of statements that ran and whether they are still applied. A partial backup read as a green
success. Both footers switch to `modal_footer_split` only while the progress line is ticking,
and each modal's body is a `flex_grow` scroll with `min_height(0)`, so a wrapped note takes its
room from the body. The
bar's own styling — full width, the modal's side padding, the top rule — is the private
`footer_bar`, which `modal_footer_split` and `modal_footer_noted` both call.
**`footer_error` is that row's left half when a draft doesn't validate**, and it exists because
there were six copies of it — byte-identical in five (`table_designer`, `trigger_editor`,
`view_editor`, `event_editor`, `routine_editor`; `object_editor` differed only in the number) and
each freezing its box at `max_width(460.0)` while the `font_size` beside it scaled, so at
**Huge** the longest messages these render wrapped to two or three lines inside a footer whose
height is fixed (see the size invariant). **It is one line, ending in a `…`, with the whole
message in its tooltip — and no `max_width` at all.** The first version kept a scaled
`max_width(460)` as a *wrap hint*, on the reasoning that `modal_footer_split` would shrink the
box and the text would wrap earlier; but wrapping is the failure, not the fallback, in a row
whose height is fixed — a second line ran behind the buttons or out of the modal, which is what
the account editor's *"Pick a level, name what it applies to…"* did under text twice
English's length. Since `modal_footer_split` already ends the status
where the actions begin, the line needs no bound of its own, and one helper serves every modal
width. Its callers grew past the six schema editors with that fix: the account form's blocker
line and the grant form's *needs a role / privileges* line (hand-rolled before, one capped at
340), `database_editor`'s status (capped at 320), `dump_view`'s and `script_view`'s error, and
the export error in `chart_view` and `result_diff_view`.
It is `ellipsized_text` in `error` at `font_label`. **`ellipsized_text(t, style)` is the
general form** — one line with `min_width(0)`/`flex_shrink(1)`/`text_ellipsis` and the full
text as a tooltip **only when the line is actually cut** — for any status, hint or label beside
controls that must not give way. Every line it drew used to carry a tooltip repeating it, cut
or not; now `on_resize` keeps the text's laid-out width in a signal and the wrap is `tip_when`,
whose closure — read at hover — compares `measure_text_px_at(full, font)` against that width
plus a pixel of slack (measuring and laying out round differently). The rule is the user's — a
tooltip only on truncated text, as the tab titles and the ERD header already behaved. The font
it measures at is read off the caller's own `style` closure
(`style(Style::new()).get(FontSize)`, `font_body` when it sets none), so a caller that sizes
its text by inheriting from a parent is measured at `font_body`. Two callers pair two lines and
set `flex_shrink(1000)` on the one that should give way first: the grid row panel's
`field_label`, whose type (once `flex_shrink(0)` beside a shrinking name) squeezed the column's
name to nothing in the fixed `scaled(150)` label column — `SMALLINT UNSIGNED` did it on a wide
screen — and the palette row's secondary, which at equal shrink shared the cut with the
command's own name (`overlays.rs`). The import modal's `mapping_row` puts the file's header
name in one, in its own fixed `scaled(150)` column, where a bare `text_ellipsis` gave no way to
read a long header at all.
The `min_width(0)` is the half that is easy to forget, and it goes **twice**: on the text, and
again with `flex_shrink(1)` on the `.tooltip()` wrapper after it, since the wrapper is the
row's flex item (*`.tooltip()`'s wrapper is the flex item* under *Floem 0.2 gotchas*). The first cut
(`2cdfbd64`) had only the first, and Manage Connections' *New*/*Import*/*Export* rows and the
Properties *Count rows* hint still overflowed under doubled text (fixed in `32a2cafb`). The
parent row still needs a definite width for any of it to bite. **`menu_row_face(icon, icon_px, label, font_size, color)`** is the
icon-plus-label face of a full-width action row under a list — Manage Connections' *New
connection*/*Import*/*Export* and Users' *New account* — the icon at its size and the label
ellipsizing to the column; both lists had hand-rolled it, and a label longer than English ran
past the column's edge (Manage Connections' is a fixed `scaled(210)`). The caller still wraps it
in its `menu_item_style` container and `in_ring_button`, **whose wrapper carries `min_width(0)`
beside its `flex_shrink(0)`** for exactly these rows: in a column the wrapper's automatic
minimum was its content, so the label inside could never narrow; in a row `flex_shrink(0)`
still holds a button at its content, so nothing else moved.
**`noted_if_empty` is applied inside `open_picker`, so nothing to choose from is an answer
rather than silence.** `popup_menu_overlay` builds a panel for any `Some`, and for an empty list
its height sums to the padding and border alone while its measured width is zero — so it clamps
to the `popup_width` floor and draws an empty bordered box under the control, which then eats the
click meant to dismiss it. Declining to open at all is worse in the way that matters: the box
looks pressable, answers with nothing, and is indistinguishable from one that is broken, which is
how it was reported the first time. So the empty list becomes one **disabled** row reading
"Nothing to choose from". `table_designer::suggest_chevron` already had that arm and its own
note; its sibling `focusable_owned_dropdown` — the wrapper behind every dropdown in the designer,
the object editor, the trigger editor, the view editor, the event editor and the routine editor —
did not, and is reachable empty in each of them wherever the list comes from a fetch or a schema
that has not loaded (*Create ▸ Table* on an unexpanded database, then Foreign keys ▸ + ▸
**References table**). Putting the arm at the **door** rather than in a wrapper's parameter list
is what makes it one answer for every picker present and future; a caller with something more
specific to say hands over a list that is already non-empty, which is what `suggest_chevron`
does and why its own wording still wins.
Also the **read-only fact panel** — `fact_section` (a heading with its rows under it), `fact_row`
(one `label: value` line) and `fact_note` (an icon-led caveat), the three views a panel of
*observed* facts is built from. They were `properties.rs`'s private helpers until `users_view.rs`
copied all three character for character, at which point the reasoning existed twice and the
values could drift apart with nothing on screen to say they had. Both modules call these now and
keep one-line wrappers over them (`properties::section`/`section_with_gap`/`detail`/`note_line`,
`users_view::section`/`detail`/`note`) for the label column and row gap that legitimately differ
between the two panels. The gap and the label width are `fn() -> f64` and not numbers, for the
reason every other length here is: one resolved at build freezes at the scale it was built at.
**The label column is measured, not a constant**: each panel's `label_w` is
`widgets::fact_label_w` over every label it can show, in the interface's language — one column
still, so the values line up, at least the old 132 (Properties) or 124 (Users) and at most
under half the panel (Properties) or 240 (Users). Fixed, a longer language wrapped every label
onto a second line (*Next auto-increment* did, under doubled text); a label
past the cap still wraps.
`fact_section_owned` is `fact_section` under a heading built at run time — the *Used by* modal's
*"Views (3)"*, where the count is the heading — and `fact_section` delegates to it, so there is
still one section view; its title is `impl Into<String>`, for the properties panel's `tr!` headings.
Also the right-hand panels' shared list chrome — `panel_search` (its *Search…* is
`panel-search-placeholder`, through `FieldCfg::placeholder_of`), `panel_group_header` (a band's
title and count; its colour and border rulings are on the function — the title ellipsizes and
the count is `flex_shrink(0)`, so a narrow band still ends in its count where it used to clip
the title mid-word) and **`banded_list`, which
makes those bands sticky**: the band under the top edge keeps its header pinned while its rows
scroll, and the next header pushes it out as it arrives. It takes `(title, count, rows)` per
band and the enclosing scroll's visible rect, which `autohide_tracking` publishes from the
scroll's one `on_scroll` (*One `on_scroll` per scroll*). The in-flow headers are measured with
`on_resize` rather than computed — rows wrap, so a band is as tall as layout made it — and an
unmeasured one stands at an infinite top so it pins nothing; what the geometry means is
`core::sticky_header::pinned`'s to decide. The copy is a `dyn_container` keyed on the band index
alone, so it is rebuilt only when the pin changes hands and merely restyled while it is pushed.
It is drawn as `first`, with its own top rule, because it stands where the first header stands
at rest — which is why the **first** header's measured height is the one the push is measured
against. **The copy lives inside the scrolled content**, absolutely placed at
`inset_top(viewport.y0 + offset)`, not in a layer over the scroll: there the scroll's own clip
trims a header being pushed out instead of letting it paint over the search box above, and a
wheel over the copy bubbles to the scroll as it does from any row.
And the **keyboard-navigation cluster**, which is the subject of the Tab gotchas below:
`FocusRing` (a modal's Tab order — `register`/`unregister`/`step_from`/`remember`/`focus_at`,
plus the `ring_step` wrap rule and its deliberate opposite `list_step`, which clamps),
`focus_root_with_ring`/`innermost_ring_root` (how the modal root and the *window* root enter
it), `in_focus_ring`/`in_focus_ring_with` (how a non-field control joins, the second for one
with teardown of its own — floem keeps a single cleanup slot), `VALUE_TAB` (where a growing
block of stops starts), and the `PopupToken`-tagged `set_open_popup`/`clear_open_popup`/
`dismiss_open_popup` slot. A field joins through `FieldCfg::focus` instead, since nothing
outside floem's editor can see a key it has.
Also `takes_pointer_focus`/`note_pointer_focus` — the report a view makes when a press puts the
keyboard on it, which is the only way `begin_pointer_dismissal` can tell a click that placed
focus from one on chrome that placed none, floem answering neither question. Spell the decorator
beside every `.keyboard_navigable()`;
`menu_return_gate::every_navigable_view_reports_the_press_that_focuses_it` fails on a site that
doesn't, and the Tab gotchas below carry the bug that bought it. **There are three one-tick
press flags, not two** — `note_pointer_on_completion`/`blur_dismisses_completion` is the third,
the completion popup's report that a press landed on *it*, which is how the editor's blur tells
a click on a suggestion from a click away from the editor. All three are set during the press's
own dispatch and cleared on the next tick, for the same reason.
**Three free predicates decide what a key *is*, and the first two were each missing their
modifier term.**
`presses(key, mods)` is "does this press a button": a bare Space or Enter and nothing else,
because the two arms that asked it used to match the logical key alone — so `Ctrl+Enter`, which
is the grid's **Commit**, pressed a ringed button instead, and with focus on the results strip's
✗ (one keypress away, `step_from` resuming where the strip was left) that ran `discard_edits`:
every staged cell edit, every pending new row and every pending delete gone, unconfirmed and
unrecoverable, under the key the ✓ beside it advertises as Commit. `steps_ring(key, mods)` is its
sibling — Tab with no `shortcuts::primary_held`, Shift being the *direction* rather than a
modifier — and it was missing the same term: floem forwards a KeyDown to the window root only if
nothing consumed it, so once **F6** put the keyboard in the results toolbar strip, `Ctrl+Tab`
stepped to the next toolbar icon and tab switching stopped, as did `Ctrl+Shift+Tab` and, on
macOS, `Cmd+Tab`. Inside a modal the same consumption is harmless, because the window root
returns `Continue` for everything while one is up — which is why the strip is the surface it bit
on, and why the rule was already written at the one Tab handler that applied it, the window
root's own backstop. Both are free functions so the two arms ask **one** question rather than two
`matches!`es that could gain the term separately, and
`press_tests::every_tab_arm_asks_the_one_predicate` holds every Tab arm in the file to
`steps_ring`.
**`strip_defers(key, mods)` is the third, and it is the one that makes `presses`' doc true.**
That doc said a modified Space/Enter is declined so the event "keeps travelling" to whatever
binding owns it — and floem does not travel: `WindowHandle::event` dispatches a `KeyDown` to the
**focused** view with `directed = true`, never up through ancestors, and then, if unprocessed,
only to the window root's own listeners. The grid's `Ctrl+Enter → commit` is a listener on the
grid *body*, a sibling of the toolbar strip, so it was on neither path: with the keyboard on the
✓, under a tooltip reading "Commit N changes (Ctrl+Enter)", the press did nothing at all. So
`in_strip_button` takes a `keys: impl Fn(&Event) -> EventPropagation` and carries the panel's
bindings itself — **for the modified keys only**, which is exactly the class `presses` declines,
so the two answers cannot disagree. A bare key stays the strip's own (Space/Enter press it,
Left/Right step it, Escape leaves it), and those three are excluded modified as well.
`grid_toolbar`'s `panel_keys` — `grid_key(gs, nrows, ncols, e)` — is the app's only `keys`
closure, passed to every icon in the cluster.
**The verdict is returned as given, and that is load-bearing.** A combination the panel does not
own comes back `Continue` and still falls through to the window root, which is how `Ctrl+Tab`
goes on switching query tabs from inside the strip — `steps_ring`'s rule, one level out. It holds
*because* `grid_key` has no Tab arm rather than by accident, which is what
`the_panel_handler_behind_the_strip_claims_no_tab` pins by reading `grid_key`'s own span in
`grid.rs` and refusing a `NamedKey::Tab` in it.
