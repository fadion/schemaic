# `ui/activity_panel.rs`

`activity_panel.rs` — the **Server Activity** right-column panel (`RightPanel::Activity`, the
footer's pulse-line toggle): the sessions on the active *connection's* server, a counts line, a
lock-wait banner and a search box, over the same chrome as the History panel. It paints
`core::activity` and decides nothing itself. Three things are worth knowing. The counts and the
banner read the **whole** snapshot while the list reads the filtered one — they are facts about
the server, and narrowing the list on screen must not make a blocked session vanish from the
tally; the list then draws only `render_slice`'s front `RENDER_CAP` of that, since this whole
container is rebuilt on every poll and five hundred rows of teardown and text shaping every two
seconds is continuous churn in a panel whose subject is load. A refused kill lands in
`ActivityUi::kill_error` and prints **above** the list rather than replacing it: routing it
through `ActivityState::Failed` threw away the snapshot someone was reading mid-incident, banner
included, and with auto-refresh Off nothing brought it back. `Failed` means "there is no
snapshot"; a kill the server declined leaves the snapshot perfectly good.
**Left-click is inert on purpose**: everything a row offers ends someone else's work, so
both kills live in the right-click menu (with *Cancel query* dimmed rather than absent on a
session with nothing running, so the menu keeps one shape), and the confirm is raised by the
*app*, not here, so no route to a kill can skip it. **The lock-wait banner's Kill is the one that
answers the keyboard**, through `widgets::key_pressable` — navigable, Enter and Space press it,
and the pointer too, which it lost for a spell when adopting that helper *replaced* its
`on_click_stop` and nothing put the mouse half back (the helper binds both listeners itself now;
*Floem 0.2 gotchas*) — because it is built by hand rather than through `widgets::action_button`, whose family is the
modal-footer one and needs a `FocusRing` this panel has none of. It carries the id it will kill in
its label: a button reading just *Kill* on a panel where several sessions are killable is the one
misread that cannot be undone. The **per-row** Kill and Cancel — a session row's and a lock
row's alike — hang off a right-click menu with no keyboard opener and are still pointer-only,
which is why `README.md`'s accessibility sentence was narrowed to claim only what is true.
**The read-only flag reaches every kill surface as a `Memo`, never as its value** — the banner's
`Kill <id>`, a session row's menu and a lock row's — and that
distinction is the whole of a bug the memo's own comment had already predicted in the imperative:
it says "the two containers below read it inside their own bodies, and a value captured at build
would freeze", and both call sites then passed `read_only.get()` from inside a `dyn_container`
*child builder*, which is not a tracking context (*Floem 0.2 gotchas*: only the key closure is
wrapped in an effect). With the poll interval set to **Off** — which the panel offers — marking
the connection read-only changed nothing: the banner's `Kill <id>` stayed in full danger fill,
tooltipped "Terminate session N", and the row menu's Cancel query / Kill session stayed enabled,
so the only way to learn the action was unavailable was to try it. The inverse was worse: built
while read-only, the button's action captured `false` and stayed inert after the flag was
cleared, producing no kill *and* no message until something rebuilt the panel. No write ever
escaped — `kill_session` refuses before the confirm — so what was missing is the half
`accept_launch`'s contract asks this side to supply, the disabled button being what *says* the
action is unavailable. The memo is now read where reading is meaningful: `.get()` in the style
closure, which re-runs; `.get_untracked()` in the press and in the tip, which want the answer at
the moment they are asked; and again in the right-click handlers that build the row and lock
menus, which is later still than the row. The clock in the title bar wears the same grey as
the refresh icon beside it: it was tinted while polling, on the reasoning that "Off" and "every
2s" look identical between ticks, but two icons a few pixels apart in different colours read as
one of them being *active* in the toggle sense. The interval is stated where a state belongs —
the menu marks the chosen row, and the tooltip says it in words.
Its dropdown is `overlays::activity_menu_overlay` and **not** a `popup_menu`: the panel is
clipped for its collapse animation, so a menu built inside it is cut off at the panel's own
edge, and a cursor-anchored popup wanders. It follows the schema tree's eye/gear machinery — an
open flag plus an anchor the icon publishes from `on_move`/`on_resize` — with the one difference
that makes it not a copy: the schema panel is against the window's *left* edge, so its menus can
hang leftward and never meet one, while this panel is against the right. The anchor is therefore
the icon's bottom-**right** corner, the menu is right-aligned to it at a fixed width, and the
result is clamped into the window on both sides. That clamp **is** the edge detection.
All three anchored dropdowns — the schema eye, the schema gear and this one — drop by one shared
`MENU_ICON_DROP`, measured from the icon's **padded box** rather than from the glyph in it: the
gear fills its box where the eye does not, and those two have always sat level, so the box is
the reference and per-icon tuning by eye is the mistake it looks like a fix for. The "gear" is
a name, not its glyph: it wears Lucide `menu` (`icons::MENU`), because the `sliders-vertical`
it wore is what every other panel's settings button shows, and those open a settings modal
where this opens a dropdown (GitHub #23).
The clock's `.style()` is applied **before** its `.tooltip()`, which is not tidiness: `.tooltip()`
wraps the view in a new one, so a style after it lands on the *wrapper* while the
`on_move`/`on_resize` that publish the anchor stay on the container inside — which then reports a
bare 16px glyph box with none of the padding around it, and the menu hangs under the glyph
instead of under the control.
Two layout facts are written into the rows because getting either wrong is invisible until
someone puts a ruler on it. **A text view sizes its own taffy node to the text it measured**, so
`width_full()` on the statement preview is not decoration — without something to be 100% *of* it
lays out as one long line and the clip merely cuts it off; the width is what it wraps against.
And **the age is pinned by `justify_between` on a two-child heading**, not by a `flex_grow`
spacer before it: `space-between` says "the last child's right edge is the content box's"
directly, where a spacer only arrives there if every base size in the row measured as expected —
and a rich text measures ~20px wider than its glyphs, which is exactly how much short of its
padding the age sat through two attempts to correct it from the outside. The identity group and
the account name inside it both **grow**, which is the other half of the same lesson: nesting
the name a level deeper made taffy measure it at min-content on some pass, and a rich text
re-wraps to whatever width it is handed and then reports the *wrapped* size — so the next pass
hands it the same small width and it stays wrapped. `schemaic@localhost` broke mid-word with
half the row empty beside it until both were told to take the space that was there.
The lock-wait card is the same lesson in the other direction: it lost its right border to
`width: 100%` *plus* a 12px margin, which is 24px too wide because margins sit outside the
width — column-flex stretch subtracts them and a percentage does not.
**A Sessions | Locks switch sits under the title** (`ActivityUi::view`, an `ActivityView`,
drawn by `widgets::segmented_tabs` at `scaled(12)` side padding and `scaled(4)` from the title,
a `scaled(10)` spacer above the search box), and the app polls only the list on show, so a
switch is a fetch of the other and nothing else. The one search box filters whichever list is
shown, so its placeholder says which — *Search sessions…* / *Search locks…*, through
`FieldCfg::placeholder_of`, a placeholder read in its own reactive label so the switch relabels
the box without rebuilding the field. The Locks view paints `LocksState` — `ActivityState`'s shape
for the other list, `Idle`/`Loaded(Rc<LockList>)`/`Failed` — in three functions. `locks_header`
is the tally, from the whole snapshot as the sessions' counts are: `activity::locks_tally`'s
`N locks`, then `M waiting` in the error colour and left out at zero (the `+` on the waits only
when they could have been cut), worded by `locks_total_text`/`locks_waiting_text` from
`activity-locks-total`/`-waiting` (`english_counts_the_locks_as_core_does`), then
"list capped" — and on `contended_only` a line saying the server lists only locks waited for or
holding up a wait, so MariaDB's short list does not read as a lock-free server. `locks_list`
filters by `lock_matches_with` and the rows' own `activity-lock-waits-for`, draws `render_slice`'s front for the sessions list's reason and counts
the rest, and says which kind of empty it is: no match, no lock waits (where only contended
locks are listed), or no other session holding a lock. A connection that could not be reached
at all fails `refresh_activity` before it asks for either list; it reports on the *sessions*
state, and **on `LocksState` too while Locks is shown** — this view draws a loaded lock list
ahead of the sessions state's error, so a tunnel dropping under it left the last locks on
screen as current. The Locks branch still reads `ActivityState::Failed` while its own state is
`Idle` — read as `Idle` alone it said "Loading…" forever with auto-refresh off (found
documenting it, before it shipped). `lock_row` is a dot, red while waiting;
the session id, "holds" or "waits for", and the engine's kind and mode; the object in mono; and
"held by …" for a wait. **Right-click offers the lock's session the same two kills as a session
row** (`lock_menu`, asked `KillFrom::Locks`), reversing a rule this entry used to state. Lock
rows offered no kill because the app's kill handler named the session it confirmed from the
*sessions* snapshot and returned silently on an id not in it — which a session seen only in the
Locks list need not be, so a Kill there would have been a button that sometimes did nothing, and
the Sessions view's lock-wait banner was the only way to end a blocker. The handler now looks
the subject up through `activity::kill_subject` with the list the kill came from, and from Locks
confirms it by its id and lock from the current lock list (`core::activity`). *Cancel query* is
dimmed on a held lock (`LockInfo::offers`: the row has no session state to say whether anything
is running), absent where the engine has no statement for it as on a session row, and both
kills are dimmed on a read-only connection and on a lock a server process holds
(`LockInfo::killable`).
**The panel's own words are the catalogue's** (`activity-*`), SQLite's
no-sessions note included — `sqlite_has_no_sessions()`, a constant before it — and so is the
Locks view's contended-only line, `activity-locks-contended-only`, a literal before it, with
`english_says_the_contended_only_note_as_before` holding the English to the old sentence. The two kill
entries are `kill_label(KillKind)`, `KillKind::label`'s words drawn here (the core-words rule
under *UI conventions*), and the line under a session's state is `session_note_text` over
`SessionInfo::note_kind`'s `SessionNote` (`activity-waiting-on`, `activity-blocks`, the ids
joined here; `english_says_every_session_note_as_core_does`). Each "N more … match — narrow the search" note is one Fluent plural
where it was a `format!` around two `text::plural`s, and
`the_undrawn_notes_read_as_they_did` holds the English to the old sentence for 1, 2 and 350.
**The counts line and the state words are worded here too, and together** (`activity-count-*`,
`activity-state-*`): `total_text` words the total from `ActivitySummary::figure` (`$figure`,
which carries the `+`) and the count (`$n`, which picks the plural), and `tally_text` words the
other three by the private `Tally` it is handed, so no translated figure sits beside an English
total. `state_word` words a row's state, and the search box matches *that*: `StateWords` words
all four once per filter pass rather than once per row per keystroke, and the filter hands them
to `activity::matches_query_with`, so a search for the translated word finds the row wearing it.
`english_counts_and_names_every_state_as_core_does` holds all of it to core's `total_label`,
`state_label` and `SessionState::label`. The lock-wait card's sentence is `lock_wait_sentence`,
which words `activity::lock_wait_said`'s `LockWait` as the one message
`activity-lock-wait-sentence` (under *The string catalogue*), held to core's `Display` by
`english_says_every_lock_wait_as_core_does` across every holder state, each age present or
withheld, and none, one or three other holders — 48 sentences. Its ages and the session row's
are `activity::Age`s worded by `i18n::session_age`, the row's withheld age an em dash.
