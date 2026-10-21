# `ui/cell_editors.rs`

`cell_editors.rs` — the **type-aware value controls** the grid edits with when a column's legal
values are already written down: the picker (a box, or the in-cell face), the `SET`'s chips and
the calendar. Every builder binds to the same `RwSignal<String>` a text field would have, so the
staging, NULL and write-back machinery around it is unchanged — a control is only ever a
different way of typing the same string, and *which* string is [`core::celledit`]'s to say
(including what each row **writes**, which is not what it reads: a boolean's row reads `true`
and writes `1`, a `SET`'s writes the whole value with that member toggled). Three shapes, each
avoiding a specific failure: **one picker for booleans and enums alike**, because a boolean's
values are as listed as an enum's and one control for both is one thing to learn — a two-row
menu rather than a switch or a checkbox, since the value has a third state ("nothing chosen
yet") neither has anywhere to stand for. It opens the app's own `menu_panel` rather than a floem
`Dropdown` because of the paint-only edge nudge documented in `settings::scale_picker`, and the
row panel sits at the window bottom where that bug lands. The **`SET`** gets chips in the row
panel because a menu closes on the first pick and a subset is picked repeatedly (in a cell,
where there is no room for chips, it uses the picker and toggles one member per opening). The
**calendar** is a panel in the window's own overlay layer (`DatePick` →
`overlays::date_pick_overlay`), placed by the pure `calendar_insets` against a `calendar_size`
that is *computed, not measured* — which is what makes its edge flips exact rather than
estimated. **Every length in that computation is the same function the panel's own style calls**
— `calendar_pad`, `calendar_gap`, `calendar_band_h`, `calendar_headings_h`, `day_w`/`day_h` and
`CALENDAR_BORDER` — because both were literals in two places and a scale sweep moved only one of
them: the style call scaled and the measurement did not, so above Normal the panel was *declared
smaller than its own contents*, which does not clip but overflows the background and the border,
and makes the edge flip an under-estimate at the same time. The border is the one term that stays
a `const`, being a hairline at every scale — the same distinction `menu_panel_height` draws, and
`calendar_size` had it backwards, scaling its boxes and treating its air as the hairline.
`day_w` is 33, not the 30 it shipped at, so the month and year above the grid have room for a
language whose month names run longer than English's — 249px wide at Normal rather than 228,
which `the_calendar_at_normal_is_the_box_the_app_shipped_with` pins. Its
`DatePick::anchor` carries **both** vertical edges of the control, unlike
`PopupAnchor::BelowBox`'s three numbers, because it flips through `widgets::box_menu_inset`:
down from the control's bottom, up from its **top**. The row panel is a strip at the bottom of
the results area, so the flip is the common case there, and a panel measured from the bottom
edge (`menu_inset`'s cursor rule) opened straight across the button — which is also the button
that closes it. `open_calendar` is the one place a `DatePick` is built, and `toggle_calendar` is the
row panel's second-press-closes wrapper over it; a grid cell opens the panel directly, one tick
after its field is built. `DatePick::on_pick` is what the two openers do *not* share: it runs
when a day or **Now** has been written and only then, so the row panel (whose field is what
commits) leaves it `None` while the cell editor stages its edit there. Every path that writes the
buffer inside `calendar_panel` leaves through the same `done` closure, which is what keeps
"closed" and "chose something" from drifting into one answer. `widgets::open_picker` is the one
menu opener — it lives there, not here, because every `<select>` in the app is one of these
pickers now (see **No floem `Dropdown`**); it anchors at the control's own
`ViewId::layout_rect` (already in window coordinates), so a menu reached by Tab doesn't open
wherever the pointer was left, recognises its own menu to toggle it closed, and closes every
other menu — which it can because `PopupChannel` carries the whole `MenuFlags` rather than the
single channel it fills. A trigger that swallows its own press owes both halves of that bargain
(see *the app's menus are mutually exclusive*), and the calendar's toggle owes it too. **Which
calendar belongs to which field is settled by the buffer, not the anchor**: one channel serves
every date field in the panel, so asking only "is a calendar open" lit every field's button at
once and let any one field's teardown close another's panel. That identity is what the grid's
`cell_calendar_up` asks too — a cell editor always binds `gs.edit_buf`, which no row-panel field
ever is. **The calendar peels its own Escape** (`peeling_escape`, wrapping the row panel's
`on_escape`): every other control's popup takes the keyboard, so the key reaches the window root
and `dismiss_open_popup` answers it — but a day, a month arrow and *Now* are not focusable, so
the field beside the calendar keeps the focus and stops the key before the root ever runs.
Escape closed the row panel out from under an open calendar. While it is up the toggle is lit,
and **it pins its own hover**: it is the calendar's dismissal then, and `field_box`'s hover tint
would otherwise repaint the accent border grey under the pointer.
**Its words are the catalogue's** (`cell-*`) — the unset picker's *Choose…*
(`pick_field`'s and `pick_cell_face`'s alike), the calendar's header (`cell-calendar-month`,
`$month` and `$year`, the year passed as text so a later number-formatting phase cannot group
it) and the footer's *Now*/*Today*. The month names and weekday initials are core's words drawn
here, by the typed-value rule: the file's own `month_name` and `weekday_initials` replace
`date::month_name` and `date::WEEKDAY_INITIALS` in the calendar (`cell-month-1`…`12`,
`cell-weekday-*`), while the week's order — Monday first — stays core's, and core keeps its
English constants for whatever else reads them. `english_words_the_calendar_as_core_does` holds
both equal to core's arrays and pins the header.
