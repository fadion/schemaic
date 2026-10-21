# `ui/grid.rs`

`grid.rs` — the whole results grid (`GridState`/`GridCtx`; `loaded_view` is the entry point,
called from the results strip's body). `editor_pane.rs` — SQL editor pane
(`query_pane` + the Ctrl+K bar, statement highlight, custom scrollbars). `syntax_view` draws
severity-coloured squiggles (red errors / amber typo warnings / style hints in
`theme::diag_hint`, the editor's comment colour — the quietest the syntax palette has, so a
join with no condition reads as neither a typo nor an error).
**The diagnostics run off the UI thread.** Both triggers — the 120 ms debounced edit path and
the effect that re-checks when the catalog changes (it tracks every node's inner `schema`, not
only the `db_nodes` list, since a schema landing, a per-database Refresh and the re-introspection
after DDL write the inner signal alone and a restored tab otherwise kept no squiggle until the
next keystroke — `the_diagnostics_re_run_when_any_databases_schema_lands`) — go through
`diagnose_in_background`, which
builds the catalog on the UI thread (`completion::build_catalog` reads the schema tree's
signals), then spawns a `std::thread` that locks the pane's `diag_cache` — one
`Arc<Mutex<intel::DiagnosticsCache>>` per pane, recovered if poisoned — and runs
`diagnostics_with`, which keeps the `:param` neutralise-then-strip handling in front of the
cache. Passes queue on that lock in order. The answer comes back through `create_ext_action`
and is set on `syntax` **only if its generation is still the pane's latest** (`diag_gen`) and
the tab still lives, so a pass overtaken by typing never paints over a newer one; the status bar
goes on reading `Tab::diagnostics`.
**The inlay hints ride the same pass** (`intel::insert_value_hints`, under `intel.rs`): the
worker computes them after the diagnostics, over the same text and the same catalog, and the two
come back as one `(diags, hints)` delivery under the one generation check — so a hint set never
lands over text it does not describe, any later edit having bumped `diag_gen`. Between passes
the `.update(…)` callback carries the pane's `hints` through each delta (`d.summary()` →
`intel::shift_inlay_hints`). An effect on `hints`, created once `inner_doc` exists, bumps its
`cache_rev`, since hints are baked into the cached line layouts (the Floem gotcha on phantom
text); it skips its first run, and `inline_diff::set_hints` leaves an unchanged set unwritten, so
a pass that finds nothing new discards no layout. Nothing gates them — there is no setting.
**The tab outliving its pane is the case the delivery's check misses**: `syntax` is the tab's,
`hints` the pane's, and a connection switch while the worker runs unmounts the pane and leaves
the tab. So `set_hints` reads with `try_with_untracked` and a disposed `hints` takes nothing —
`with_untracked` there unwrapped on the dead signal and crashed the app.
**A failed run's squiggle is `run_diag`, a sibling of live validation's `db_diag`** rather than
an entry in it, because validation `set`s `db_diag` whole and would wipe it. Every launch — the
selection and lone-statement Ctrl+Enter, the run menu's two entries, the play button — records
the buffer ranges it sends in `run_spans` (Run Current's through `sql::span_in` over
`executable_at`'s slice), and an effect that tracks only the shown result places
`intel::run_error_diagnostic` there when it is a statement's `Failed` and not frozen. It reads
`run_spans` **untracked**, and that is load-bearing: tracked, the next launch's new spans would
re-place the previous run's error in them before the new result arrived. The next edit clears
both, as it does the error bar. Both merge sites that read `db_diag` — `diag_hits` and the
menu's `statement_problems` for *AI fix* — read `run_diag` too.
**The record says which launch it describes, and a result is placed in it only if that launch
made its panel.** Every launch goes through one `record_launch(spans, sent, batch)` closure,
which stamps `run_mark` (`RunMark { first_panel, batch }`) with `ShownResult::launch_mark` — the
tab's `next_panel`, the id the run's first panel is about to take — and the error effect and
`fix_error` both ask the pure `launch_spans(mark, shown_id, spans)` which spans the shown panel
ran in. In a batch `begin_run` numbers the panels consecutively, so `shown - first_panel` is the
statement's index and the answer is that one span; a single launch — one text, possibly over
several spans, as the whole-buffer send is — answers with all of them, for its first panel
only; and any other panel, or no record, answers empty, which makes `run_error_diagnostic`
place nothing and `run_fix_range` fall back to its guess. Both halves were review findings. A
run started outside the pane — the command palette's Run, the AI panel's code-block Run —
left the record as it was, so Run Current on `SELECT 1;` followed by a palette Run whose
`SELECT nope FROM t;` failed drew the squiggle under line 1's `SELECT`. And Run Everything
handed both intel functions every span, so the token search picked a healthy statement naming
the same column, or placed nothing, or widened the AI fix to statements the batch never
reached. The edit handler clears `run_mark` with `run_spans`. A run the write guard holds and
*Run anyway* releases keeps its stamp, since no panel is made in between (`launch_spans_tests`,
three, red against a stub returning every span).
**Hovering a squiggle shows its message, and nothing about the squiggle is hoverable** — a
`.tooltip()` on each one never fired and cannot be made to (the *Floem 0.2 gotchas* pointer
bullet says why). `editor_area` watches `PointerMove` with `on_event_cont` and hit-tests the
position, already in its own coordinates, against `diag_hits`: one `DiagHit` per on-screen
diagnostic, whose box is `diag_hit_at`'s — `underline_seg_at`'s x and width, so the pointer finds
a message exactly where a wave is drawn and nowhere else, from the line's top down to the wave's
bottom. The drawing memo `segs` is *derived* from `diag_hits` and drops the message, so a
message-only change still rebuilds nothing drawn and the wave and its hover box are one
computation that cannot drift apart; the geometry's two perf rules (`content_x` once per list,
the document never copied — `track`ed, not `get`) moved onto `diag_hits` with their measured
comments.
**Every overlay's points come through `editor_points`, which tracks `screen_lines` and never
makes floem lay a line out.** An overlay that needs a line's *rows* as well as its position
takes the layout from `layout_if_built` beside it — `Lines::try_get_text_layout` at the current
`cache_rev`, `None` for a line floem has not built, never building one. The Ctrl+K preview's
`inline_footer_y` (the verdict bar's y) and `inline_band_runs` (the gutter and padding strips of
the inline-diff bands) were the two exceptions until R2.1-L8-01: each tracked `screen_lines` by
hand and then called `points_of_offset` and `text_layout` raw, and `visible_hunk_lines` gave the
bands the range check alone, not the layout check that stops the crash. Both now take
`editor_points` and `layout_if_built`; a line not laid out yet is skipped this run (a band) or
answers `None` (the footer), and the layout pass that rewrites `screen_lines` re-runs them.
`geometry_tests::a_screen_lines_reader_never_lays_a_line_out` holds the rule over the crate's
source — a `fn` whose body reads `screen_lines` through `.track()`, `.get()` or `.with(` must
not contain `points_of_offset(` or `.text_layout(`, `editor_points` alone excepted — and was red
naming exactly those two. It reads one body's text, so a tracker reaching a layout through a
helper in another `fn` passes it. The tracking is the fix for drift: `points_of_offset` reads
`screen_lines` untracked, so an overlay whose closure tracked only `viewport` re-ran with last
frame's line positions, its boxes slid a few rows onto unrelated text, and — the tell — they did
not come back when the editor scrolled back. But a tracker must not ask about a line with no text
layout: `points_of_offset` builds the layout, building one fires floem's `LayoutEvent`, and the
effect answering it rewrites `screen_lines`, which re-runs the asker from inside its own run —
"RefCell already borrowed" in `floem_reactive`, and the app gone. Every edit opens that window,
since bumping `cache_rev` drops floem's layout cache (`Lines::check_cache`) until the next layout
pass, and the viewport `Rect` set in `EditorView::compute_layout` ran `diag_hits` inside it:
typing into a 7 MB script of ~70k statements crashed. So the lookup is the pure
`points_on_screen`, which answers `None` without asking floem for an offset whose buffer line is
outside `screen_lines`' `rvline_range` or has no layout yet (`Lines::try_get_text_layout` at the
current `cache_rev`); the layout pass rewrites `screen_lines` and the caller re-runs with the
layouts in place. **The layout check is the one that stops the crash** — floem already answers
`(ZERO, ZERO)` early for a line outside its screen lines — and the range check only spares a walk
of `screen_lines` per diagnostic in a long script; drop the layout check as redundant with the
range check and the crash is back. `points_on_screen_tests` pins both refusals with floem never asked.
`diag_under` settles an overlap —
the offline pass and the server's verdict on the same token — the stronger severity by
`Severity::rank` (error, warning, hint, as `dedup_diagnostics` sorts), then the narrower span, inclusive on the left and exclusive on the right. `hover_diag` returns before
touching a signal on a move that changes nothing, because every pointer move lands there and a
set notifies even when the value is equal; otherwise it records the candidate in `diag_pending`
and shows it after `DIAG_TIP_DELAY` — 300 ms, the app-wide `.tooltip()` delay set on
`TooltipContainerClass` at the root, so every tip in the app answers at one pace — behind a
generation counter read with `try_get_untracked`, since the tab may close inside the delay. It
offers nothing while the completion list is open, **nor where an overlay covers the text**.
Each view `editor_area`'s stack draws over the editor — `error_bar`, `guard_bar`, `cmdk_view`,
`run_menu_view`, `find_bar`, `goto_bar` — is wrapped in `marks_pointer`, which sets a shared
`Rc<Cell<bool>>` from its own `PointerMove`, and `editor_area`'s listener reads and clears it in
one step (`replace(false)`) and hands `hover_diag` `None` when it was set. That leans on floem
0.2's dispatch (`context.rs`): a pointer event goes to the topmost child whose rect contains the
point, the child loop then breaks, and the parent's own listeners run after — so the overlay has
always marked the move by the time `editor_area` reads it, and the flag never outlives the
dispatch that set it. **Both listeners stay `cont`**: an overlay that consumed the move would
keep `editor_area` from hearing it at all, and a tip pending when the pointer crossed onto the
overlay would still open. Before this the hit test saw straight through every one of them — the
run menu's full-pane catcher included — and the tip, at the window-global z of 1001 (below),
painted over whatever covered its squiggle.
`diag_tip_occlusion_gate::every_overlay_over_the_text_marks_the_pointer` holds those six to the
wrapper **by name**, so a new overlay added to the stack is not caught until it joins the list.
`PointerDown`, `PointerLeave` and `KeyDown` retire the tip, and so does an effect on any change
of `diag_hits` (a scroll, a new set of diagnostics), of the document, of `comp.open`, of
anything that comes up over the text (`run_menu`, `cmdk.open`, `find_open`, `goto_open`,
`error_msg`, `guard`), of the caret (`ed.cursor`) and of `editor_focused`. **The caret and the
focus are what catch the keyboard, not the `KeyDown` listener**: floem hands a key to the
focused view and then to the window root, never to an ancestor between (*Floem 0.2 gotchas*),
so `editor_area` hears effectively no key at all — the listener's own comment, that it sees
"the ones the editor does not consume", is wrong, and the listener is close to dead — while a
key the editor takes moves the caret and a modal a shortcut opens blurs the editor. The delay also refuses to show while
`widgets::innermost_focus_root()` is `Some`, for a keyboard route to a modal or menu that never
reached this pane to retire it. `diag_tip_view` is paint-only, styled with
`widgets::tooltip_style` and placed by `diag_tip_place`, below the line in the pane's upper half
and above it (`inset_bottom`) in the lower half, so a long message is never cut by the bottom
edge. **Horizontally it pins an edge (`TipX`) rather than sliding one**, because the tip's real
width is its message's and unknown until layout: its left edge at the squiggle's start, growing
rightward up to `DIAG_TIP_MAX_W` (480, scaled) or the pane's edge less `TIP_EDGE`, whichever
comes first — or, when less than `DIAG_TIP_MIN_W` (240, scaled) is left that way and the other
way has more, its right edge (`inset_right`) at the squiggle's end, growing leftward. The first
spelling slid a left edge far enough to fit the *maximum* width, which moved every short message
too: in a ~640px pane every squiggle right of ~150px opened its tip at ~150px, under whichever
error on the line came first (`each_squiggle_gets_its_tip_under_itself_when_there_is_room`).
Tests in `geometry_tests` pin the rest — `the_hover_box_is_exactly_as_wide_as_the_squiggle`
holds the hover box to the wave, and
`the_tip_stays_inside_the_pane_margins_wherever_the_squiggle_is` sweeps pane widths and squiggle
positions for either edge ever reaching past a margin.
**Alt+Enter applies the quick fix under the caret** (`Diagnostic::fix`, under `intel.rs`). The
key handler's arm — Alt, neither the primary modifier nor Shift — asks `intel::fix_at` over the
pane's `syntax` diagnostics alone, the offline pass's, since nothing the server reports carries
a fix, and writes the answer over that diagnostic's range through `edit_untyped`: one undo step,
refused under the Ctrl+K freeze, the caret left after the replacement. **Only on the text the
pass described** (`intel::current_fix_at`): `diagnose_in_background` sets the pane's
`syntax_text` to the text beside `syntax`, and the key compares it with the buffer. A pass lands
a debounce and a parse after the keystroke and nothing shifts its ranges meanwhile, so typing
`e` into `selct` and pressing Alt+Enter at once spliced `select` over `selec` — `selectt`
(`a_fix_for_older_text_is_not_offered`). `syntax_text` starts empty, so a tab's diagnostics
kept from before the pane mounted fix nothing until the pane's own pass lands. **The key is spent even
with nothing to fix**, so a stray Alt+Enter never falls through to a newline. The tip speaks
the interface's language: `tip_message` words the diagnostic's `what` (`intel::DiagMessage`)
through the private `diag_message_text`, and a T-SQL control-flow error through
`flow_error_text`, from `diag-*` — a message per variant, `diag-table-not-found` and
`diag-column-not-found` taking a `$within`/`$suggested` selector (`some`/`none`) for the
optional *in …* and *did you mean* parts, the row-width messages a plural per count, a reserved
alias a message per `AliasQuote` — while the parser's detail passes through as it came, and a
server error shows its `shown` wording where it has one and its English `text` where it has
none (`a_server_error_hovers_in_its_wording`). Both producers set it: the run-error squiggle
maps `run_error_diagnostic`'s answer through `shown_as(err.displayed())`, and the app's live
check `db_error_diagnostic`'s through `shown_as(&db_error_shown(&e))`, so a German interface
hovers *Abfrage fehlgeschlagen: …* around the server's own text, which stays the server's.
`english_says_every_diagnostic_as_core_does` holds it equal to core's
`DiagMessage::message` for every variant: the not-found pair with each optional part present
and absent, alone and together, counts at one and several, every quote and every
`FlowError`. The tip says what
the key would do: `diag_hits` builds each message through `tip_message`, which appends a line
`Alt+Enter: <fix>` (the keycap through `shortcuts::keys_label`) holding **`fix_at`'s answer at
the squiggle's start**, over the same offline list — never the hovered diagnostic's own `fix`.
The two pick differently: the tip shows `diag_under`'s strongest diagnostic of every source, the
key `fix_at`'s strongest *fixable* offline one, so a server error over a fixable offline warning
once showed the error alone while Alt+Enter still applied the warning's fix. Now the error's tip
names that fix (`a_server_error_over_a_fixable_token_still_names_the_fix`,
`the_tip_shows_the_fix_the_key_would_pick_not_its_own`).
**The same tip is a name's quick-info**, and Ctrl+click opens what the name names — both over
`core::quickinfo`. `DiagHit` carries `name_at: Option<usize>`: where `diag_under` finds no
squiggle, `hover_diag` asks `name_box_at`, which undoes `content_x`, `EDITOR_TEXT_TOP` and the
viewport to reach a document point, takes `Editor::offset_of_point` only when that lands inside
the text, and boxes the run of word bytes on the line — so a diagnostic on the same word wins.
It runs on every pointer move and so reads nothing document-sized: the code column comes from
the rope's line count through `content_x` (below), not from the text. **Resolution waits for the delay** — `quick_info`
(`completion::loaded_schemas` → `name_at` → `info` → `quick_info_text`) runs only when
`DIAG_TIP_DELAY` fires, so a pointer crossing a line of words resolves none of them, and a name
that names nothing shows no tip rather than an empty one. `quick_info_text` words core's
`QuickInfo` from `quickinfo-*` — *table*/*view*, a column count as a plural whose `$n` is core's
ungrouped figure in the interface's marks (`i18n::figure`), each column trait, *function*/*procedure* with and without *returns* — while names,
types, defaults, generation expressions, signatures and comments pass through, and the
`name  type` line and the ` · ` between traits stay as core writes them. It used to show
`describe`'s English in every language; `english_says_every_quick_info_as_core_does` holds it to
`QuickInfo::message` for three tables — a table at one column and at 1,200, neither with a
comment, and a view at three columns with one (no view at one column, and none without a
comment) — a column with every trait and with none optional, and both routine kinds with and
without a return. Being the same `diag_tip`, it keeps everything above — the
overlay gate, what retires it, its placement. The `Copy` hover closure reaches the `Editor`
through a `RwSignal` (`ed_qi`). **Ctrl+click** is the editor's outer `PointerDown`, which runs
after floem's own click has put the caret under the pointer: the primary button with the
primary modifier and neither Alt nor Shift asks `ctrl_click_target` at the caret and hands a
`CtrlTarget::Object` to `QueryPaneParams::open_object`, built once in `lib.rs` by `open_named_object` — a table
or view through `open_table`, a column through `open_table_col` (which scrolls to it), a routine
through `object_editor::open_for_object` with the loaded `RoutineInfo`, the doors the schema
tree and Find Anywhere use. Alt+click is left to floem and still adds a caret; a Ctrl+click on
nothing resolvable just places the caret. **Nor does one past a line's end, or below the last
line**: floem puts the caret just after the line's last word there, and clamps a point under
the last line onto it, both of which `name_at` accepts, so the branch first asks
`pointer_offset`'s `inside` of the last point the `editor_area` PointerMove saw
(`last_pointer` — the click arrives in the editor view's coords, which `pointer_offset` does
not take) and opens nothing outside the text, as the hover shows nothing there
(`ctrl_click_asks_whether_the_pointer_is_on_the_text`).
**A name the statement defines itself jumps instead of opening** — an alias, a derived table, a
CTE. `ctrl_click_target` asks `intel::definition_at` *before* `named_object_at`, answering
`CtrlTarget::Defined` or `CtrlTarget::Object`, and the order is load-bearing:
`quickinfo::name_at` resolves an alias to its table, so asked first it opens `orders` for a
click on `o`, which is not where `o` is. A `Defined` hit closes completion, selects the
definition (`Selection::region`) and `center_window()`s; on the definition itself
`definition_at` answers `None`, so a click there still opens what the name names.
**Holding the primary modifier underlines what a Ctrl+click would act on**, because without it
which words were clickable was, in the user's words, a guessing game. That is why the
resolution is one function rather than the click's branch: `ctrl_click_target` was lifted out
of `PointerDown` so the click and the underline ask the identical question, and a word is
underlined exactly when the click would go somewhere — a column, an alias, a table, but not a
keyword or a database qualifier, which the click does nothing on either. The pane's
`update_ctrl_link(p, held)` asks it while the modifier is held — on a move, under the click's
own rule of neither Alt nor Shift with it — and the completion list is closed, at **the
pointer's own offset** (`pointer_offset`, the offset floem's click puts the
caret on), not the word's start, so the two cannot disagree at a word's edge. The word's bounds
are `word_under`, split out of `name_box_at` (which now calls it) so the quick-info box and the
line agree on where a name starts and ends. `ctrl_link` holds the word and its
`underline_seg_at` segment; a move within the same word at the same place re-asks nothing, and
the signal is written only on a change, since every pointer move lands here. `ctrl_link_view`
draws it as a 1px `theme::accent()` rule at `y + floor(WAVE_H / 2)` — where a squiggle's wave
sits — paint-only, in `syntax_view`'s slot (*Floem 0.2 gotchas*: the stack is at its 16). The
tip-retire effect clears it on the same triggers, as it is placed in the same coordinates, and
`PointerLeave` clears it along with `link_pointer` — the underline's own record of the pointer,
`None` while an overlay (find bar, menu) covers the text, so a Ctrl pressed there underlines
nothing beneath. **Pressing or releasing the modifier with
the pointer still has to move the line too, and no listener on `editor_area` can hear that**:
floem gives a key to the focused view and then the root, never the ancestors between, and the
editor consumes a lone Ctrl. So `shortcuts::modifier_keys` is noted where the key does
arrive — the editor's own key handler, the root's `KeyDown` (focus elsewhere) and a root
`KeyUp` (the editor takes no key-ups) — and an effect in each pane re-asks at `link_pointer`
when `opens_link` goes true and clears the line when it goes false, so Shift or Alt held with
Ctrl, before or after it, draws nothing. The effect reads a memo of `(primary, opens_link)`,
not the signal, so a bare Shift while typing does not re-run it and re-arm a retired tip. A
release while the window is unfocused never arrives, so a pointer move corrects it
(`shortcuts::note_pointer_modifiers`: Shift and Alt as the pointer reports them, the primary
modifier only ever cleared) — otherwise the next press would change nothing and draw nothing.
**Over the underlined word the pointer is an arrow**, not the I-beam, since the click there
opens something rather than placing a caret: an effect beside `ctrl_link_view` writes the
editor's `pointer_cursor` — `CursorStyle::Default` while `ctrl_link` is `Some`, `Text`
otherwise. That signal is vendored floem's seventh patch (*Floem 0.2 gotchas*), because
`editor_content` set the I-beam in the text view's own style, which no ancestor class outranks,
and `ctrl_link_view` is click-through, so it cannot wear a cursor of its own. **And no hover
tip while Ctrl is held** — the pointer is hunting for something to open, and a box over the
text only hides it: the `editor_area` move asks `hover_diag` for nothing while the modifier is
down, the effect that follows `modifier_keys` retires a shown or pending tip on the press
and re-arms the delay for the word under the pointer on the release — that call untracked, as
its word lookup reads `screen_lines` and a scroll re-running it would bring back a tip the
scroll had just retired.
**Alt+Shift+drag is a box selection** (`text_ops::box_regions`). `name_box_at`'s way back from
a pointer to the document is factored out as `pointer_offset` — the offset under the pointer and
whether it is inside the text, `None` over the gutter — which the drag shares. *Inside* is
floem's own past-the-line's-end answer **and** a `y` above the bottom of the line it landed on:
floem's alone let the blank space under a one-line query underline, tip and arrow the word
above it (`pointer_offset_tests`, on a headless `Editor` built as floem's own tests build one). The same outer
`PointerDown`, again after floem's own click (which with Shift+Alt would extend the selection or
add a caret instead), sets `box_anchor` at the pointer and puts a lone caret there on the
primary button with Alt and Shift; any other press clears it, as does a `PointerUp`. **That
press also hands floem's pointer capture back** — `clear_active()` on `ed.editor_view_id` — and
the box depends on it: the content view calls `request_active()` on every press, an active view
is sent every move directly, and so the listener below never heard one; the drag stayed floem's
stream selection and Alt+Shift+drag drew no box in the running app. The clear works only
because it is queued behind the content view's own request (*Floem 0.2 gotchas* has why). With
the capture gone a release outside the pane reaches neither `editor_area`'s `PointerUp` nor
floem's, so `PointerLeave` ends the box as well, clearing `ed.active` — floem's "button held",
which only its `pointer_up` resets — or moving back over the text with no button down kept
drawing one. The outer
`PointerMove`, while an anchor is set and floem's `ed.active` says the button is still held,
**replaces the selection whole** with `box_regions(query, anchor, head)` on every move —
floem's own drag handling runs first and is overwritten each time. Unlike the hover, this does read the document: `box_regions` walks the query's line
starts on each move, but only while a box is being dragged. Alt+click and Alt+drag without
Shift stay floem's. The view side has no test; it was build-only, which is how the capture
above went unnoticed.
**The right-click menu's *Copy as single line*** puts `sqlfmt::single_line_copy`'s answer on the
clipboard — the selection, or with none the statement under the right-click, on one line (the
`sqlfmt.rs` entry has what that keeps and what it refuses). It is **absent rather than dimmed**
when that answers `None`, as *Create view* is, and re-derived when clicked, like every other
action in the menu. The selection it reads is the one floem's `right_click` leaves: a click
inside the selection keeps it and a click outside collapses it to the caret the click placed,
so what is copied is the statement under the pointer unless the pointer was on the selection.
It has no shortcut. **Two in-place entries follow the same rules** and write through
`rewrite_range`, one `edit_untyped` (a single undo step, refused under the Ctrl+K freeze):
*Make single line* writes `sqlfmt::single_line_in_place` over the same text the copy takes, and
is absent where that is already one line; *Format statement*, placed straight after *Format
code*, writes `sqlfmt::format_statement` over the statement under the click, and is offered only
**without a selection** — with one, *Format code* already formats just that — and only when the
statement is not formatted yet, keyword case included. A selection stays selected over the rewritten range; a bare
caret keeps its place among the tokens (`sqlfmt::caret_after_replace`). Each has a palette
command (*Format Statement*, *Make Single Line*) and a key (Ctrl+Alt+Shift+L, Ctrl+Alt+J), which
act at the caret through `Tab::format_req` — an `Option<FormatReq>` naming which of the three
formats is asked — and the same `sqlfmt` answer and `rewrite_range`. **The menu entries wear no
keycap on purpose**: they act on the statement under the click, the key on the one at the caret,
so a keycap would claim the key does what the click does.
***Rename alias*** is offered where `intel::alias_name_at` answers at the right-click, or failing
that `intel::output_alias_name_at` — one entry for both kinds of alias, a table's first —
absent otherwise, and re-derived when clicked, like the rest — and is appended after the entries
above (*Find usages*, below, straight before it), ahead of *Create view*'s group. An output
alias's binding rests on which columns the loaded schema says the `FROM` sources have, so the
entry carries no catalogue: `completion::build_catalog(db_nodes, active_db)` is called each time
one is asked for — when the menu is built, and again when Rename is pressed. It has no key. It used to open no modal either: it set the
cursor to one `SelRegion` per `alias_occurrences` range and let the next word typed replace every
one at once, which left the name nowhere a check could stand — renaming the outer `o` to an inner
query's `i` silently rebound a correlated `o.id` (R3-L5-03, under `intel.rs`). It now sets
`OverlayUi::alias_rename` (threaded in as `QueryPaneParams::alias_rename`) to an
`AliasRenameRequest`: the name whichever of the two read, and an `apply` that re-runs
`intel::alias_rename` — `output_alias_rename` where the click found an output alias, the kind
being fixed then — against the buffer **when Rename is pressed**, at the offset the menu was
opened on, and writes the `MultiEdit` through `edit_untyped_multi` — one undo step — plus a
`back` that returns the keyboard to the editor when the dialog closes (*A menu the keyboard
opened gives focus back*, under *Popup menus*, says why the menu's own return cannot). Its
`Err` is
the refusal the dialog (`alias_rename.rs`) shows, and two of them are decided here rather than
by the walk. **`Changed`**: the click also captures the query text, and Rename refuses when the
buffer differs from it — the stored offset was taken in that text, so after an edit under the
dialog (an agent's, say) it may sit on another alias than the one the field names, and the
rename would go through under the wrong name. **`ReadOnly`**: where `edit_untyped_multi` refuses
(the Ctrl+K freeze) the refusal is returned rather than dropped — dropped, the dialog would close
as if it had renamed. ***Rename CTE*** comes right after it, offered where
`intel::cte_name_at` answers, and is the same entry over `intel::cte_rename` (under `intel.rs`):
the same dialog titled for it, the same `Changed` and `ReadOnly` answers, the same `back`, and
the same one undo step through `edit_untyped_multi`. The multi-region editing the old entry leaned
on stays: Backspace and an auto-paired `(` used to collapse the
regions to the primary one; they now act at every caret (`text_ops.rs`, *Several carets*), as
do Tab, Shift+Tab, Ctrl+/, Ctrl+D, Alt+↑/↓, Ctrl+Shift+U, Ctrl+Shift+J and Ctrl+Shift+/.
***Find usages*** sits straight before *Rename alias*, offered where `intel::usages_at` answers
at the right-click with the same catalogue — a table alias, a CTE or an output alias (under
`intel.rs`) — and re-derived when clicked. It sets `OverlayUi::usages` (threaded in as
`QueryPaneParams::usages`) to a `UsagesRequest` (`usages_view.rs`): the name, the rows
`intel::usage_lines` makes of the spans, the same `back` as the renames, and a `jump` that
selects the usage's range and calls `center_window()`, as Ctrl+click's *Defined* arm does. **The
rows' offsets are the ones taken at the click**, so `jump` holds the text they were taken in and
does nothing once the buffer differs — an edit made under the open list (an agent's, say) would
otherwise have a row select whatever those offsets now hold, or past the end; Rename's `Changed`
is the same check.
**The Ctrl+K suggestion is not drawn here** — it is phantom rows in the editor's own line flow
(`inline_diff`, below), and what `cmdk_popup` still draws is the two ends of the block: the
question bar (sparkle, a `multiline` `edit_field` capped at three rows, then a send affordance or
the `verb_spinner`) anchored under the statement, and the verdict footer — Accept / Reject / the
change count, read off the *published* plan so there is
no second diff of the same text — anchored under the added rows. Neither covers the editor any
more, so the compact↔expanded animation went with the box it was growing.
**That count is `inline_plan_summary_text` drawn by a `label` closure, and the closure is the
point.** The function is `diff::inline_plan_summary` restated in the interface's language —
`cmdk-no-changes`, and `cmdk-diff-summary` a plural on the hunk count with every count core's
ungrouped figure in the interface's marks (`i18n::figure`) — held to core's English for no plan, an empty one, one hunk and two
by `english_says_the_inline_plan_summary_as_core_does`. The footer's container is keyed on `verdict_shape` — `cmdk.open` and whether `inline_ai`
is `Ready` — which never reads `cmdk.preview`, so the sentence built in the builder's body
subscribed to nothing and would freeze at whatever the plan said when the bar last became
`Ready`, while the editor's own phantom rows moved on (`inline_band_runs` is a *key*, so it does
track it). Stale counts beside a correct diff is the shape of it. Nobody saw that happen: it read
right only because the effect that writes `preview` derives it from the same `inline_ai` variant
the key already tracks — an invariant this view neither states nor depends on, which makes it a
latent bug rather than an observed one.
`cmdk_preview_gate::a_preview_read_sits_in_a_tracking_scope` scans this file so every
`cmdk.preview` read stays inside one.
**The overlay's `dyn_container` is keyed on a `Memo` of `(open, is_ready)` — not on the state,
and not on a bare closure**, and both halves of that are load-bearing. Keyed on the state, every
Idle → Busy → Failed transition rebuilt the question field, and a freshly built `edit_field` has
almost no width for one frame, so a one-line question wrapped in that frame and the field latched
the row count it measured there. The bar reopened a row taller with its text stranded at the top,
which looked exactly like a typed newline and sent three rounds of fixes at the Enter key (see
`edit_field`'s Enter contract for the misdiagnosis). Computing the pair in the key closure did
**not** fix it, because `dyn_container` does not diff — it swaps the child on every *run* of the
key, whatever the value (`create_updater` → `swap_val`), the same trap the Live Monitor's body
documents. Only the memo, which notifies solely when the value differs, actually spares the
field; a fourth round of Enter fixes went to the wrong place for want of that distinction.
Everything that differs across those states is therefore reactive *inside* the branch: the
trailing slot (send triangle vs `verb_spinner`) is a nested `dyn_container`, the failure message
is another, and `on_submit` reads the state when pressed instead of being rebuilt per state.
**That trailing slot reserves the spinner's width while the triangle is showing** — its verb is
picked once per opening and the slot takes `loading_dots_w` of it, `justify_end` so the triangle
keeps the right edge. The field beside it flex-grows into what is left, so without the
reservation submitting swapped a 15px icon for up to 102px of "Metamorphosing..." and took that
width off the question mid-flight — a second, quieter way to the same re-wrap.
A sent question is **dimmed and takes no more typing**, through `FieldCfg::frozen` — a `Memo`
the *built* field reads, not a second spelling of `read_only`. That distinction is the whole
point: `read_only` and the text colour are resolved when the field is built, so following the
request through them meant rebuilding it, which is the re-wrap above. Both were dropped for a
while on the reasoning that editing mid-flight is harmless (the request carries the text it was
sent with) — but an editable box promises an edit that cannot reach the question already asked,
and the spinner alone was left saying so. `frozen` drives the editor's own `read_only` signal
through an effect (floem re-reads it per keystroke in `TextDocument::receive_char` /
`run_command`, so there is no second gate to fall out of step with it) and dims the field's own
colour to half inside its reactive style. **The focus effect has to honour it too**: an AI fix
opens the bar already `Busy`, and focus arrives after the freeze, so a focus that reset the
caret put a blinking one in a field that takes no keys. The input
row also carries **no vertical padding of its own**: `edit_field` already puts `chat_pad_v` above
and below its text, and the second helping made a one-line question sit in a bar deep enough to
read as two.
An **effect** over
`inline_ai` publishes and clears the preview rather than a set inside each transition: it maps
`Busy` to an `InlineView::Working` over `diff::line_span` of the captured range and `Ready` to a
`Plan`, and *everything else to `None`* — every way out of `Ready` (approve, reject, Escape, a
second Ctrl+K, a tab switch cancelling the generation) has to take the rows down, and those
share no single caller. The buffer read sits in a closure **only those two arms call**: Idle and
Failed are the common transitions, and they were copying a whole 190 KB script out of the rope to
compute offsets they then discarded.
**With nothing selected, Ctrl+K widens to `sql::statement_range`**, the same "what does this key
act on" answer Ctrl+Enter gives; it used to capture a bare caret, so an unselected Ctrl+K asked
the model to edit an insertion point and left the diff nothing to replace. It reads the text for
that from **`e.doc().text()`, not the `query` signal** — the offsets it is resolving are the
document's own, and `accept` re-reads the document for the same reason. The right-click *Ask
AI* entry widens, selects and anchors identically: it is the same action reached another way,
so it has to pick the same thing. Ctrl+K then **selects** that range for real
(`cursor.set_insert`), which is where the design's selection colour behind the statement comes
from: the honest way to show what the key picked is the editor's own selection rather than a
lookalike overlay, and it leaves the user able to see, extend or replace the range with the
gestures they already have. The bar is anchored at the **end** of the acted-on range rather than
at the caret, so it sits under the whole statement instead of splitting one in two, and every
entry point goes through `anchor_cmdk` to do it.
**That sentence was not true of *Optimise*, and `cmdk_open_gate` is what makes it a fact rather
than an intention.** Four entry points open the bar — the key, *Ask AI*, *Optimise*, and
`fix_with_ai`'s three menus — each written separately, and *Optimise* selected nothing and
anchored nothing. The two symptoms did not look like one bug. With no selection, the in-flight
fade over the acted-on lines was the only sign of what was about to be rewritten, so the gesture
that reads as "this statement" everywhere else read as *the editor going dim*. With no anchor,
the bar opened at whatever `cmdk.point` the previous Ctrl+K had left — the origin on a fresh
tab, so it drew **over** the statement, and pressing Ctrl+K once anywhere and closing it again
"fixed" it for the rest of the tab's life, which is the tell for a stale anchor and reads as
nothing else. It also dropped its `highlight_pick`: the statement border belongs to the action
whose answer lands *elsewhere* (*Explain*, which sends prose to the chat panel and needs
something in the editor pointing back), while an action that rewrites the statement in place has
the selection as its marker. The gate is a source gate for the reason the crate's others are —
the thing under test is a set of call sites — and it bounds its look-back to a window rather
than latching a flag per file, since a 5.8k-line file has unrelated `set_insert`s in it.
**`cmdk_anchor_at` is `anchor_cmdk`'s pure half, and it exists because
`points_of_offset` answers `(Point::ZERO, Point::ZERO)` for an offset it cannot place.**
`screen_lines` is built with no overscan, so anything outside the visible range falls into that
arm — and the anchor consumed it as a position. A 300-line `INSERT … VALUES` with the caret on
line 1 has `statement_range`'s `end` far below the viewport, so the bar anchored at the
document origin: unscrolled that is y = 9, *over* the first line of the statement it is meant
to sit under; scrolled down 2,000 px it is −1,991, and `editor_area` neither scrolls nor clips,
so the prompt opened **off the top of the window with the keyboard already in it** and every
keystroke went into a box nobody could see. The scroll fix below could not rescue it either —
from a deeply negative anchor it answers `None`. The likelier route in is *AI fix*, whose
button sits on a bar pinned to the pane's bottom and which anchors to a failing statement that
need not be on screen at all. So an unplaced `end` falls back to the **caret's** line, which is
on screen by definition and is where the user is looking, and an unplaced caret (nothing laid
out yet) to the top of the *viewport* — never to the document's origin, which is not visible.
`anchor_cmdk` does two things neither caller should
repeat. It stores the point in the editor's **content** coordinates and leaves the style closure
to subtract the viewport, so the bar tracks a later scroll — while the closure was *not*
subtracting it, an open in a scrolled editor placed the bar by how far down the document the
statement is rather than by where it is on screen, which put it at the bottom of the pane. And it
**scrolls the editor when the bar would not fit** below the line: the bar is an overlay, so the
editor does not know it exists and will leave the anchor line flush against the bottom of the
pane, opening the prompt clipped or entirely out of sight. `CMDK_BAR_RESERVE` is the room it asks
for, deliberately generous — over-scrolling by a few pixels is invisible, and
`scroll_beyond_last_line` guarantees there is somewhere to go.
**The AI fix is one action with three ways to ask for it**, `fix_with_ai`: the error bar's *AI
fix*, the error modal behind its *View* (through `Tab::fix_req`), and the right-click *AI fix*
over a squiggled statement. All three open Ctrl+K pre-filled and go straight to `Busy`, so the
user approves or rejects a diff and nothing runs. The caller supplies the range, because only the
caller knows which statement it means — the run error's comes from `intel::error_fix_range` (the
failing statement, not the buffer), the menu's from `sql::statement_range` at the right-click
with `intel::problems_in_range` for the messages, core's English whatever the interface's
language — and `prompt::ai_fix_prompt` supplies the
words. It **selects the range** as the other two entry points do, and this is the one that needs
it most: the range is `error_fix_range`'s choice rather than a gesture of the user's, so without
the selection nothing distinguishes "rewriting this statement" from "rewriting all forty".
The menu entry **is only there when that statement has a diagnostic**, shown rather than
disabled like *Create view*, but the build-time pass decides only *that*: the range and the
messages are re-derived when the entry runs, like every other action in this menu. Captured,
they could outlive the text they described — a reload between the right-click and the click
leaves `sql.get(lo..hi)` answering `None`, and the entry then does nothing with nothing said.
The modal's is the narrower case: it appears over an error marked `ErrorSource::Statement` —
the tab's run error it falls back to, or an `error_modal_text` override — never over a
`Server` one (a commit error, a server that didn't answer) or an `App` notice, neither of which
is anything the editor can rewrite (`core::model::ErrorModalContent::resolve`). No surface
marks an override `Statement` today — the grid bar hands the modal `Server` for a failed commit
or re-run and `App` for its own notices, since neither a commit nor a re-run is a statement in
the buffer — so in practice the fix is the fallback's alone, and only when the run's own
failure was a rejected statement: the fallback is no longer a statement's by construction (the
`core::model` entry has what used to reach it). The bar's *AI fix* follows the same answer. It
has to route
through a request signal at all because `CmdK` is created inside `query_pane` and never leaves
it, so the workspace-level modal has no handle to reach it with — and that signal **carries the
message**, because the modal shows the error it opened on while a run landing behind it moves
`results` out from under that (its dismiss layer stops clicks, not queries). It also **closes
before it asks**: signals notify synchronously, so the request opens Ctrl+K and focuses its field
on the spot, and asking first left that focus to be decided by the teardown of the modal's own
`focus_root` afterwards.
**`Explain` is the fix's pair, and it deliberately goes somewhere else**: a fix is a diff in the
editor, an explanation is prose in the chat panel — the same split the right-click menu already
makes between its own *Explain* and *Optimise*. It reveals the panel before sending, because a
message into a hidden panel reads as a button doing nothing, and it highlights the statement it
asked about, so the answer and the SQL it is about are visibly the same statement. Both the error
bar and the modal offer it, on the same `ErrorModalContent::resolve` answer, and both send a
statement with the question only when the error also earns a fix — the modal's rule, which the
bar's `explain_error` now shares, so over a refused connection the bar neither sends nor
highlights a statement the error is not about. What either sends is the error's `text`, English
whatever language the bar and the modal show it in (`ModalError::shown`, under `core::model`).
On the bar the two AI actions sit **together at
the far edge**, which is what the sparkle marks — *View* opens a window, those two reach a model — and the bar is
**responsive about them**: `error_bar_fits_explain` measures every gap and label to the right of
the message and asks whether they fit in the share the message's own `max_width_pct` leaves them
(`ERROR_BAR_MSG_PCT`, 60/40), dropping *Explain* when they don't. It is the one of the three that
can go, because the *View* modal offers the same explanation. **A share, not a pixel floor**, and
the first attempt was the floor: a minimum width for the message with the buttons taking whatever
was left. It passed a bar where the message ended up a few ellipsized words with the buttons
packed against it — the arrangement it existed to prevent — because what makes the bar look
crowded is the buttons' *proportion* of it, and a floor written at 100% is the wrong number at
every other interface scale anyway. Two smaller things had to be true for the drop to be a
*choice* rather than the layout's opinion: the buttons are `flex_shrink(0)`, and the message
carries `min_width(0)` — a flex item defaults to `min-width: auto` and refuses to shrink below
its content, so a long error did not ellipsize past its cap, it pushed, and the right-hand
buttons were drawn on top of one another. The bar's two horizontal insets are **one
`padding_horiz` on the bar** rather than a margin on each end's child: written as two margins
they were the same number and still did not read as one, because the right-hand button is a row
(icon, gap, label) whose box ends past its last glyph. Unlike the fix it needs **no request
signal**:
the chat panel belongs to the workspace, so the modal can reach it directly, the way the schema
tree's own *AI explain* does. It is also offered **where the fix is not** — over a `Server`
error, an override or a tab whose connection was refused, where there is no statement to
rewrite but the server's words still deserve an answer — and withheld in two places: over an
`App` error, Schemaic's own notice or a run it refused to send, which already says everything a
model could (it used to be offered there too, on "connections.json was missing", "Not
connected to X" and, from the tab, "connection no longer exists" alike); and when the modal was
opened on nothing at all, where it would ask the model to account for the phrase "No error.".
The bar withholds both buttons on the same answer, and both on a kept result as well
(`ShownResult::frozen`); *View* it always shows, since the message itself is still true.
`inline_footer_y` is the other end's geometry, and it is deliberately not `points_of_offset` at
the anchor line's end: that offset maps to a column *before* the phantom rows, so its `bot` is
the bottom of the line's own row. The **next** document line's top is the honest answer — the
added rows are exactly what pushed it down — and the last-line case, which has no next line,
steps past them with `layout_if_built(ed, anchor)?.line_count()`, **visual rows and not `add.len()`**:
with word wrap on a long added line occupies more rows than it has lines, and counting lines put
the bar one row short with the tail of the suggestion stranded below it. It also takes `area_h`
and answers `None` when the bar would not fit inside the pane — it is absolutely positioned in
the pane rather than clipped to the editor, so a block scrolled past the top left Accept/Reject
floating over the toolbar and the tab strip. That fit test measures the bar through the same
`VERDICT_BAR_H` the style draws it with, since a height living in two literals is a bar that
reports itself on screen at one size and paints at another. Inside it the padding is deliberately
**top-heavy** (1px border, then 7 above the words and 1 below): the row is centred in the content
band, so an even split centres the words' *line box*, and a line box carries descender space that
almost nothing in "Accept · Reject · 1 hunk" fills — geometrically centred reads high.
**The verdict bar takes pointer events and forwards the wheel.** It lies across the document,
and Floem's child walk `break`s on the first view eligible for a pointer event whether or not it
handled anything (the pointer-routing gotcha), so a bar with pointer events on kills
scrolling over itself and *not* handling the wheel does not help. It forwards `PointerWheel` to
`ed.scroll_delta` instead — which is exactly what Floem's own gutter does with the same problem
(`view.rs:1112`) — and that is what lets it keep its buttons and set `CursorStyle::Default` over
them, since it sits on the editor's I-beam and nothing in it is selectable. (It was briefly
split in two, surface in the click-through overlay and words here; forwarding is the better
trade, and `pointer_events(false)` is now reserved for the band strips, which have nothing to
click.) **The forward is on all four views** — the row, Accept, Reject and the change count —
because the walk breaks at whichever child is under the pointer: on the row alone, a wheel over
the two words still died, which reads as intermittent rather than as a missing handler. The bar
is aligned to the code column, and the same two closures answer the keyboard: `CmdK::verdict`
publishes them to the editor's key handler, because in that state there is no field left to catch
Enter and Escape and the editor is what holds focus. One accept and one reject in the pane, two
ways in.
**Which states the editor answers for is `cmdk_editor_keys`, and Escape's are not Enter's.**
Enter belongs to `Ready` alone — there is nothing to accept before the suggestion lands — but
Escape also takes down a request that is still `Busy`. A request is not always started from the
prompt field: *Optimise* and the three ways to ask for an AI fix open the bar already `Busy`
from a menu the user clicked, so the editor never gave the keyboard up. Gating the branch on
`Ready` therefore left Escape doing nothing for the whole of a running request — it closed the
bar while prompting (the field's own `on_escape`) and while previewing a diff, and not in
between, which is the state a user most wants out of. `Idle` and `Failed` stay the field's:
both have a live, focused, unfrozen field that answers Escape itself, and a branch here would
take the key from the completion popup the user can open in the editor underneath.
**The forward is also on the outer box**, and that is what the "sometimes it swallows
the scroll" report was really about: the row is content-height, so the bar's padding, its border
and the space above and below the words all belong to the box, and a wheel worked across the
words' own band and died everywhere else. The centring is the same story from the layout side.
It is **`justify_center()` on the inner `container(content)` wrapper** — the one that actually
fills the box's definite height, and whose default `justify_start` was pinning the row to the
top. Putting it on the absolutely-positioned box did nothing, because that box's child chain asks
for `height_full` and so already fills it; the row's own `height_full` + `items_center` had
nothing to resolve against either. Centre on the view that fills the height, not on the one that
defines it.
**Both bars run edge to edge**, and that is a **deliberate divergence from the design**, which
insets the question bar from the right: both belong to the block of lines they sit under, and one
of the two stopping short read as an inconsistency rather than as a detail. It costs something,
which is why the scrollbars moved — see below. They are inset 1px on each side to clear the
editor's own border, and the verdict bar carries 2px of vertical padding. Those 1px insets are
`EXEMPT` entries in `consts::float_inset_gate` rather than `float_inset()` calls, for the reason
the entries state: these are not floating boxes keeping air from a panel edge but rows of the
block above them, and the number is the border's width, which does not scale.
**The buffer is frozen while a suggestion is on screen** (`ed.read_only`, released on accept or
reject — and read the gotcha on that flag before touching it, because the auto-pair handler owns
it too and it does not gate `Document::edit`): the phantom rows are anchored to line *numbers*
and the plan was computed against the text as it was, so an edit underneath would leave the rows
describing lines that had moved and Accept
splicing at stale offsets. The old overlay got that for free by covering the editor; this
one deliberately does not, so the freeze has to be asked for. For the same reason **a click in
the editor dismisses Ctrl+K only while it is still `Idle`**. Treating any click as "never mind"
was safe only while the working and diff states covered the editor and a click in them could not
land there at all; neither covers it now, and the diff in particular sits *in* the lines, so
clicking the very thing being decided on threw away a generation the user had waited for.
`inline_band_runs` finishes those bands, visiting the lines the diff touches **that are on
screen** (`visible_hunk_lines`) and asking
`inline_diff::row_split` — the same function the code column's own bands go through — which of
that line's **visual** rows are the block's and which are its own, so the two halves of a band
cover the same rows even where word wrap has given a line more rows than it has lines. Its
`top_of` answers `None` for a line **past the end of the document** as well as for one off
screen: the plan is computed against the buffer as it was, so a hunk can outlive the lines it
names, and clamping such a line to the last one (as it used to) painted its band against
whatever text happens to be there now. **`row_split` alone does not make the halves agree**:
it answers *which rows*, not *which lines*, and the deletion band needs the second answer too —
`hunk.del.contains(&line)`, the same gate as `sql_highlight`'s `replaced`. Where that is
load-bearing is the **pure insertion**, whose visited line is the anchor: an untouched context
line the block merely hangs off. Ungated, the strips painted a red band and a `−` in the gutter
beside a line nothing had happened to while the code column stayed clean — the two halves
agreeing about rows and disagreeing about lines, in exactly the case the `del.is_empty()` branch
exists for. Reading `row_split`'s guarantee as covering more than it does is the same shape of
mistake as the row count it replaced: **a helper that answers most of a question invites the
caller to stop asking the rest.**
`sql_highlight`'s `LineExtraStyle` covers the code column correctly, but the
editor's content lives inside a clipping `scroll` whose gutter is a *sibling painted before it*, so
nothing drawn from inside can reach the gutter or the wrapper's right padding. Those two strips
carry no text, so an overlay finishes the band across them without covering anything — and the
left one carries the row's `−`/`+` in place of the line number it hides, which is what the
design puts there. The gutter strip stops **`HL_PAD` short of the code column** — the margin the
statement-highlight *border* pads outward by to clear the glyphs; a filled band reaching the
column would sit on the first character. (It was added when the column was the constant below
and ran generous, so the band was painting over that character outright.) The overlay is
`.clip()`ed to the editor for `inline_footer_y`'s reason — line geometry against a surface that
scrolls under it — and is nested with `editor_box` inside the `editor_area` stack (which takes
at most 16 children, and the pair occupies the same rect either way), sitting **directly over
the editor, under every other overlay**: the overlays above it carry text and must not be
covered.
**The code column is measured from the code font, never a constant** — `content_x(ed)`, which
every overlay here reads: the statement border, the run outline, the bracket and occurrence
boxes, the squiggles and their hover boxes, the run menu's anchor and fold, these strips, the
Ctrl+K verdict bar's left padding and the quick-info hit test. It was `HL_GUTTER = 56`
("measured") plus `HL_DIGIT_W = 8` per extra digit, and neither follows the font: it fitted at
about 20pt, and at the default 14px the statement border and the occurrence boxes sat a few px
into the first glyph. It is now what floem's gutter sizes itself to
(`vendor/floem/src/views/editor/gutter.rs`: left padding, the last line number's measured width,
right padding) past `editor_box`'s 1px border — the pure `content_x_for_lines(total_lines,
digit_w)`, over the rope's line count — with the digit measured by `gutter_digit_w` as the gutter
measures its numbers: floem's `TextLayout`, the editor's own family, the same rounded size,
cached in a one-entry `thread_local` on (size, family), since a pointer move and a scroll frame
both ask. `digits × digit_w` assumes the code font sets digits tabular. **The editor sets both
gutter paddings explicitly** (`gutter_left_padding(GUTTER_LEFT_PAD)`,
`gutter_right_padding(GUTTER_RIGHT_PAD)`) because `content_x` adds them up: drop the left call
and the gutter falls back to floem's 25 while the sum keeps 14; the right one equals floem's
default of 30 today, and is set all the same so the sum reads the app's number, not upstream's.
The font size is a reactive read, so `content_x` inside a style closure, a memo or a
`dyn_container` *key* moves with a zoom — and inside a `dyn_container` *builder* does not, floem
0.2 calling the builder untracked (the builder-freeze gotcha). The bracket and occurrence boxes
took `let cx = content_x(&ed)` there, and the statement border computed its whole
`statement_line_boxes` there, so a zoom left all three where it found them until the caret moved
(GitHub #20). The two box lists now read a `code_x` memo built *beside* their `dyn_container` —
still once per change rather than once per box, so `no_overlay_builder_derives_the_code_column_per_element`
holds — and the statement border's key *is* its boxes, `highlight.get().map(|(lo, hi)|
statement_line_boxes(..))`, so the font, the line geometry and the scroll all rebuild them. That
last part is the trade: the border now follows a scroll as well, by rebuilding its few boxes on
every scroll frame while it is up. `no_overlay_freezes_its_geometry_in_an_untracked_builder`
refuses a `content_x(` or `statement_line_boxes(` after the builder's `move |` in those three
blocks, and only those three — it names them, so a fourth overlay is not covered.
**A memo of the code column also tracks `query`**: the font size is a tracked read, but the line
count `content_x` widens by is the rope's, read untracked (`Editor::rope_text` → `doc()` →
`get_untracked`), so a memo of `content_x` alone re-ran on a zoom and never on an edit — once a
query gained a digit of lines (9→10) every bracket and occurrence box sat a digit-width left of
its text until the next zoom. The squiggle memo already tracked it for that reason;
`every_code_column_memo_follows_the_line_count` refuses a `create_memo` reading `content_x(`
without `query.track()`, file-wide. The f64 dedups, so the boxes restyle only when the column
really moves.
`the_code_column_follows_the_code_font_s_digit_width` and
`the_code_column_widens_with_the_line_number_digit_count` pin the sum; `content_x_of` survives
only under `#[cfg(test)]`, the geometry tests' fixed origin at a nominal 8px digit, and
`HL_DIGIT_W` only as the inline-diff marker's inset. The statement border's left clamp is the
column's **margin**, `content_x - HL_PAD`, not the column: clamped at the column, a line
starting at column 0 lost its left `HL_PAD` and the border sat on the first glyph — hidden while
the generous constant put it *in* the glyph instead (`a_statement_at_column_zero_keeps_its_left_margin`,
red before).
**The custom scrollbars paint *above* the Ctrl+K bars**, listed after `cmdk_view` as **two
separate children** — the stack is at exactly 16, which fits. The bars run edge to edge and the
scrollbars are pinned to the editor's *border* rather than to its content edge, so at their old
layer an open Ctrl+K covered them; being drawn last costs the bars nothing (the scrollbars are
thin and sit at the extremes) and puts a drag where the user aimed it. **Never wrap the pair in
a `stack` to save a child slot**, however tight the 16 gets: a wrapper around them is
`absolute().inset(0)`, a pane-sized view above the editor that takes pointer events, and it ate
every click, drag and wheel in the editor for as long as it existed (see the pointer-routing
gotcha, which states the general form).
**The squiggle tip found its slot the other way round**: `signature_popup` and `diag_tip_view`,
both paint-only, share one child as `stack((signature_popup(..), diag_tip_view))`,
`absolute().inset(0)` and `pointer_events(false)`, so the stack stays at 16. The wrapper carries
`z_index(1001)`, the signature hint's own layer: in floem 0.2 `z_index` is not a sibling order
but the renderer's paint layer, set when a view paints and inherited by its descendants
(`context.rs`'s `save`/`restore` of `z_index`) — the same fact the completion popup's
`z_index(1000)` relies on to overhang the results pane — so the wrapper's value reaches the
squiggle tip and lifts it over the bars, the scrollbars and the results pane; the hint's own
`z_index(1001)` in `completion.rs` agrees and keeps it right wherever it is placed. That is
a pane-sized wrapper above the editor — the very shape the previous sentence forbids — and it is
harmless only because it is click-through and neither child has anything to hover. The same
layer is why the tip must never *open* where something covers its squiggle: 1001 is above every
bar in the stack, so a tip raised under the run menu or the find bar paints over it, and
`marks_pointer` (above) is what keeps it from being raised there.
**The run outline marks what Ctrl+Enter would run before it is pressed** — a 4px bar set
back into the gutter (`content_x - HL_PAD - 12`, 11px clear of the code — a 2px bar hard
against it read as a caret), `theme::query_highlight()` at 0.6 alpha — a muted accent tint, not
a red, which read as an error — beside the lines of `run_outline`'s range. An effect over the caret (`ed.cursor`),
`query` and `dialect` recomputes it on every move and edit through the pure
`run_outline_range`: `StatementIndex::run_current_range`'s piece — the pane's index, from
`completion::stmt_index`, answering as `sql::run_current_range` does — or `None` with a
non-empty selection (Ctrl+Enter runs the selection instead) and wherever `run_menu_wanted` sees
no choice to make, so a lone query carries no mark. That is the menu's own question, so the bar and the menu agree
about when there is anything to tell apart. The set is guarded as `ident_occurrences`' is, so a
caret moving within one statement rebuilds nothing. The bar asks for points only on the
statement's lines that are on screen — its line span intersected with `screen_lines`'
`rvline_range` — so a scroll frame costs the same for a statement of three lines or three
thousand, and the code column comes from the rope's line count (`content_x`, asked inside the
style closure so a zoom moves the bar), never from a copy of the text: the range's end follows
the text, so the bar rebuilds on most keystrokes in a script, and `content_x_of(&query.get_untracked())`
cost ~1 ms of each at 4 MiB. The run menu, the Ctrl+K band, the bracket boxes and the
occurrence boxes read it the same way, the last two through a memo beside their container
(`no_overlay_copies_the_document_for_the_code_column`, red before). Its top is
`EDITOR_TEXT_TOP` like every overlay's, with no nudge of its own: the `+ 1.0` it carried "to sit
flush with the current-line band" was the border pixel every overlay was missing (GitHub #9, the
*SQL editor padding* gotcha), and re-adding one now puts the bar a pixel low. `the_run_outline_marks_what_ctrl_enter_would_run` pins the range, and was red
against a core without the blank-line cuts (under `sql.rs`). **It shares a slot with
`highlight_view`**, the run menu's statement border, as `stack((highlight_view,
run_outline_view))`, because `editor_area`'s tuple is at floem's 16: an `absolute().inset(0)`
wrapper with `pointer_events(false)` — the pane-sized shape the scrollbar note forbids, harmless
here for the squiggle tip's reason, being click-through with nothing in it to hover.
**It asks undebounced on every move, so it asks the index, not the walk.** Handing
`scan_bounds_with` the caret as its `until` was the first fix and was not enough: with the caret
mid-file the walk still measured 6.3 / 25 / 104 ms at 1 / 4 / 16 MiB per move (release), where
the index answers in ~0.0001 ms (under `sql.rs`); a caret in the tail no terminator ends finds
that tail's blank-line cuts once per revision. The effect reads `query.with`, not `get`, so a
caret move copies no document. Ctrl+Enter and the run menu still walk, one call per press.
**`visible_hunk_lines` is a filter, never a clamp.** A line outside the viewport is dropped, not
pulled to the nearest one — the rule `top_of` already states for a line past the end of the
document, and for its reason: a band placed against whatever text happens to be at the clamped
line is worse than no band. It returns the hunk's deleted range narrowed to the visible lines
plus the anchor of a pure insertion when *that* is on screen, since an insertion has no deleted
line to hang its added rows off. `None` for the viewport is an editor with nothing laid out yet,
which places no offset at all. The narrowing is why it is a function: `inline_band_runs` re-runs
on **every scroll tick** and used to call `points_of_offset` for every line of every hunk before
finding out the line was off screen — floem answers that with a linear scan of `screen_lines`.
A whole-buffer suggestion, which `fix_with_ai` produces routinely whenever
`intel::error_fix_range` cannot locate the error's token, therefore cost `O(deleted × visible)`
per frame with none of it drawing anything; bounded by the viewport it is `O(visible)`.
**A known cost, accepted rather than fixed**: `inline_band_runs` returns pixel positions and is
the strips' `dyn_container` key, so every scroll frame rebuilds those views while a diff is on
screen. Making it reactive means keying on `(line, row)` and moving the geometry into N per-strip
style closures that each call `ed.text_layout(line)` — a real refactor with its own failure
modes, for a transient overlay of usually under ten rows. The per-call cost dropped a long way
when the strips moved to `block_at`, which is what made leaving it the reasonable trade.
**The find bar's hit-recompute effect has two reasons to run, and they are split because only
one of them is worth a frame.** It tracks the document deliberately — an edit while the bar is
open moves every later match, and the hit list is what Replace edits *by*, so reading the
document untracked meant a hit computed before an edit was used after it: typing `-- ` at the
head of a query turned `SELECT a FROM t;` into `-- SELECT a FRx t;`, destroying the `OM` of
`FROM` while the `t;` the user searched for was left alone. The effect's previous value is the
query it last ran for, which is what separates the two. A **new query** recomputes now and
reveals a match, because the user is watching `n/total` and waiting to be taken there;
a delay in that path is a find bar that feels broken. The match is the first at or after the
selection's start, or the caret (`text_ops::first_hit_from`, over `Hit`s), not match 0: a needle Ctrl+F
seeded from the selection stays on that occurrence, and one typed a character at a time stays
on the match in view instead of leaping to the top — VS Code's behaviour. **While the bar is
closed the effect returns an empty string, not the query it ran for**, so reopening on the same
needle is a new query too. Carrying the old one over sent that reopen down the edit path below:
no reveal, and a `find_idx` still on whichever occurrence the user had last stepped to rather
than the one just selected. It reads the haystack with `query.with`
rather than `get`, since the scan borrows the document and cloning a 16 MiB buffer to hand to a
function taking `&str` was 2.4 ms of a 20.3 ms keystroke. An **edit** rides a 120 ms
generation-counter debounce, the same one the diagnostics pass beside it uses (the live-DB
validation next to that is 500 ms) — rescanning the whole document synchronously is 17 ms at
16 MiB, on the UI thread, on every keystroke. That is safe rather than merely cheaper because
nothing reads `find_hits` until Enter, an arrow or Replace, and **`replace_one` revalidates
against the live document** instead of trusting the list ("one stale offset here rewrites text
the user never searched for"), so a list that lags a tick cannot become an edit. The empty-query
arm still calls `query.track()`, or typing a needle after clearing one would read a document the
effect had stopped following.
**Match case and Whole word are part of the query, not a filter on its hits.** They are a
per-pane `find_opts: RwSignal<FindOptions>` (`text_ops`, above), its `dialect` filled from the
pane's at every read rather than stored, held for the pane's life like
the needle beside them and not persisted, and the effect's previous value is
`(query, opts, scoped)` rather than the query alone — so flipping one is a new query, rescanned
and revealed at once, not an edit left to the debounce. `scoped` is the *in selection* scope's
**generation**, `find_scope_gen`, which `set_find_scope` bumps when the user sets or drops a
scope and the edit that carries one does not: keying on the bounds would send each keystroke
inside the scope down the new-query path, and keying on whether one is set made a move from one
scope straight to another an edit — no reveal, and the old scope's hits up for a tick. **A new query bumps `find_gen`
too**: it was bumped by the edit path alone, so an edit's 120 ms tick still out when a new
needle — or now an option flip — landed would fire after it and put the old needle's hits back
under `n/total`. Both paths scan through one `scan` closure over `hits_in` (the import alias
for `text_ops::find_hits`, the signal having that name), which publishes a pattern's compile
error to `find_error` or clears it, and the hits are `Vec<Hit>`, so the reveal and the step
select each hit's own length.
`replace_one`'s revalidation and its rescan, and
`replace_all_cb`, take the same options — through `hit_holds`, `find_hits` and `replace_hits`:
revalidating by the default rule would let a hit Whole word had refused pass as a fold-equal
span and be replaced, and under regex `hit_holds` asks for the same span, not merely a match at
the same start. **All three read the text the search read** — `hay` up to the scope's end,
which `hit_holds` and `replacement_for` take the scope for — so a greedy hit the scope cut short
holds, and fills `$1`, with what it covers rather than with the longer match the whole document
would give (`a_hit_the_scope_cut_short_is_the_same_hit_everywhere`). `replace_one` then expands the replacement with `replacement_for` — so `$1` is
the hit's group — rescans within the scope, and advances past `off + expanded.len()`, the
replacement as written being the wrong length once a group has been filled in.
**The *in selection* scope** is `find_scope: RwSignal<Option<(usize, usize)>>`, held apart from
the editor's selection, which revealing a hit replaces, and carried through each delta in
`.update(…)` by `text_ops::shift_range`, beside the inlay hints' `shift_inlay_hints` and a
snippet's tab stops' `TabStops::shift` (`Completion::stops`, under `tab_stops.rs`). A delta that
encloses the scope drops it — set straight to `None`, not through `set_find_scope`, so *Sel*
goes out and the find effect's debounced edit path rescans the whole text without a new
query's reveal moving the caret — and one that encloses a stop ends the walk. *When* the
stops are set is load-bearing: floem calls the update callbacks
synchronously inside the edit (`on_update` in its `text_document.rs`), so a snippet's own
splice runs the hook before `begin_snippet` has set the stops it creates, and never shifts
them by the body's own length — setting the stops ahead of the splice would. Ctrl+F and
Ctrl+H make a multi-line selection the scope (`find_scope_seed`); anything else clears it on a
bar being opened but **keeps it on one already open**, since there — Ctrl+H over Ctrl+F — the
selection is the hit a step revealed, not a new block. **That hit is not read at all**
(`text_ops::selection_seeds_find`, given the bar's open state, the selection and
`find_hits[find_idx]`): a regex hit can span lines, so it would otherwise become the scope —
"Sel" lit, the count `1/1`, All replacing one hit — or, on one line, its text would replace the
pattern that found it. Closing (× or Escape's `CloseFind`)
clears it. **Replace All in a scope edits only the scope's span**, `new_text` cut to it by the
unchanged tail's length: an edit over the whole document, the ordinary Replace All's, would
enclose the scope and lose it through `shift_range`. The toggles are four text glyphs right
of the find field, `Aa`, `W`, `.*` and `Sel` (tooltips *Match case*, *Whole word*, *Regular
expression*, *Find in selection*), lit `chip_active` when on and `chip_idle` when off — the
status bar's toggle colours — which is what took the bar's content width from 283 to 331 with
the first two, and to 389 with all four. `Sel` is not an `opt_toggle`, the scope not being a
`FindOptions` field: pressing it takes the editor's non-empty selection, and pressing it again
drops the scope. A pattern that does not compile shows **Invalid** in `theme::error()` in the
count's place, with `find_error`'s sentence as its tooltip — `pattern_error_text` of core's
`text_ops::PatternError`, `find-bad-pattern` or `find-bad-pattern-why` with the regex crate's
reason passing through as `$why`, held to core's `Display` in English by
`english_says_a_bad_pattern_as_core_does`. None of them has a key.
**Replace All honours `edit_untyped`'s refusal before it touches `find_hits`**, which is the
guard `replace_one` beside it already had. `replace_all_cb` wrote the new hit list unconditionally
and it was computed from `new_text` — text that, when the editor is frozen, was never written. For
an ordinary replacement whose replacement does not contain the needle that list is **empty**, so
the bar reported no matches, `n/total` collapsed and Enter did nothing, over a document that still
held every occurrence: the screen read as though the replace had happened.
**The editor's key order is one decision, `editor_key_plan`, and the view only carries it out.**
It used to be a run of `if`s in `text_editor_keys`, each reading signals another had just set,
so the order — which *is* the behaviour — had no test. The tab navigation (Ctrl+P/T/W/Tab/1–9)
and the Ctrl+K verdict stay ahead of it in the closure, unchanged, and Ctrl+Space onwards after
it. The handler builds a `KeyCtx` — the key as an `EdKey` (←, Home, End, PgUp and PgDn are one
`Move`), the modifiers, which bars and offers are up, the *shown* ghost's offset
(`inline_diff::shown_ghost`, below), `lone_caret`, and whether the caret ends its line, which it
works out only for a → with a ghost showing — applies the returned `KeyPlan`'s `dismiss` and
`close_list`, then performs its one `KeyAction`. Escape closes the find bar, then Go-to-line,
then turns down signature help, the list and the ghost, and only with none of those on screen
ends a snippet's tab-stop walk (`StopEnd`, spent; `KeyCtx::stops`, under `tab_stops.rs`); an
open list with rows takes a bare ↑/↓, Enter and Tab, and any other ↑, ↓, → or `Move` closes it
and goes on; then, with stops being walked, Tab without the primary modifier is `StopNext` and
Shift+Tab `StopPrev`, closing the list — behind the open list's Tab, whose word may be what is
going into the stop, but ahead of everything below, since the user is filling a template in;
then Tab takes a ghost and
→ a word or the whole of it, ahead of Tab's indent, Enter's carried indent and Shift+Tab's
outdent, which closes the list as a caret move does. **The clearing that falls through is
modelled inside the function, not left to the signals**: after an Escape that turned down
nothing on screen, after a caret move that closed the list, and after Enter closed it on a word
typed in full, the later checks see the list (and, for Escape, the ghost) gone — which the view
got for free by setting a signal and reading it back, and a plan that only named the action
would not. An open list with *no* rows takes nothing and stays open, so Tab over it is passed on
to floem rather than indenting (`tab_over_an_empty_open_list_is_passed_on`). The order is pinned
in `ghost_key_tests` (`escape_closes_find_then_goto_then_dismisses`,
`an_open_list_owns_its_keys_and_a_caret_move_closes_it`,
`a_ghost_takes_tab_and_right_ahead_of_the_indent`, `enter_indents_and_shift_tab_outdents`,
`a_snippets_stops_take_tab_and_shift_tab`,
`escape_ends_a_snippets_stops_after_what_is_on_screen`); what
stays in the view is the edit each action makes, and Enter's own fall-through to floem when the
caret is not one region. For the stops that is: select the range `forward()`/`back()` answers,
or on `Step::Finish` clear the stops and put the caret at the snippet's end, or on `StopEnd`
just clear them. **The walk also ends when the caret leaves it** — an effect on the cursor
clears the stops once the lone caret, or a selection's higher end (`max` of the two, where a selected stop ends), is no longer one
`TabStops::holds` — so a click elsewhere or an arrow out of the last stop does not leave Tab
to jump back into a template the user has moved on from.
**Enter is a newline once the word is typed in full.** With the list open, a bare ↑/↓ moves the
selection (`list_takes_arrow`: with Shift, Alt or Ctrl the arrow closes the list and does its
own work — Shift+↓ used to walk the rows instead of extending the selection) and Enter or Tab
accept `items[sel]` — except Enter with `comp.word_complete` set and
the selection still on row 0, which closes the list and lets the key through to the pane's own
newline (`text_ops::newline_indent`, under `text_ops.rs`).
The exact match is never a row (under `core/rank.rs`), so after `FROM` the list held
`FROM_BASE64()` and `FROM_DAYS()`, and Enter, meant to end the line, accepted the first. An
arrow onto a row first still accepts that row, and Tab still accepts row 0. The handler reads
only where the selection is, not whether it moved, so ↓ then ↑ back onto row 0 is a newline
too. The flag is under test (`a_word_typed_in_full_is_reported_though_not_listed`), and so,
since `editor_key_plan`, is the key decision
(`enter_after_a_full_word_is_a_newline_unless_a_row_was_chosen`).
**Tab takes the inline prediction ahead of the indent, on `ghost_takes_tab`'s terms**
(`ghost_key_tests`): a plain Tab — no Shift, no primary modifier — with the list closed, since an
open list owns Tab, and `completion::lone_caret` still answering the offset the ghost was made
for (`ghost_at.is_some() && ghost_at == caret`, the caret being `lone_caret`'s `Option`).
Anything else falls through to the indent. With several carets or a selection there is no lone
caret, so Tab indents — each caret by its own indent, in one edit (`text_ops.rs`) — and Ctrl+→
moves every caret
(`no_lone_caret_leaves_both_keys_to_what_they_always_did`): both keys used to compare
`cursor.offset()`, the last-inserted region, so a ghost was inserted at one caret of several
and `set_offset` collapsed the rest. The insert goes through `edit_untyped`, so the Ctrl+K
freeze holds and `suppress` keeps the popup from reopening over the inserted words, and the caret is put after
them; a refused insert still spends the key. **Ctrl+→ (Cmd+→ on macOS) takes one word of it**,
on `ghost_takes_word`'s terms — Tab's, but with `primary_held` rather than without it, so
Shift+primary+→ still selects by word and a plain → still moves
(`primary_right_at_the_predictions_caret_takes_a_word`,
`a_word_is_taken_only_by_primary_right_at_a_live_prediction`) — **except at the end of its
line**, where a bare → has nowhere to move the caret and takes the whole ghost, as the shells'
autosuggestions do (`ghost_takes_right`,
`a_bare_right_at_the_line_end_takes_the_whole_prediction`); mid-line it moves, and the ghost
left behind goes. The word is
`intel::prediction_word_end`'s in the editor's dialect (so a quoted name goes whole, under
`intel.rs`), inserted through `edit_untyped` as Tab's is; the rest goes back
as a `kept` ghost at the new caret, stamped with the document's length after the insert, and is
**published last**, after every caret change the insert makes — so the effect that clears a
ghost whose offset is not the caret never sees it at a stale one. Escape turns a ghost down as it
does the list and signature help, consuming the key only when one of the three was up — and a
ghost that is up is one `InlineDiffDoc::ghost_on` will draw outside a Ctrl+K preview, now that
`completion::pick_prediction` refuses a prediction holding a line break before it is
published; until then Escape could be spent on a ghost nobody could see.
**All three keys ask `inline_diff::shown_ghost`, not the slot** — the ghost as it is on screen,
none while a Ctrl+K preview is up and none holding a line break, the one predicate
`InlineDiffDoc::ghost_on` draws by too. Tab used to read the slot, and so took a ghost hidden
behind a preview, inserting words nobody could see and spending the key the preview was
waiting on. Escape still clears a hidden ghost, but consumes the key only for one that was
shown, so the preview's own Escape is left to answer. The key handler wants the preview
untracked, and `cmdk_preview_gate::a_preview_read_sits_in_a_tracking_scope` refuses an
untracked read of `cmdk.preview` by name, so the handler asks `preview_up`, a memo over it,
untracked instead; the gate now counts two reads, the verdict footer's and that memo's.
**A ghost lives only at the caret it was made for**: an effect on `ed.cursor` clears it the
moment `lone_caret` stops answering its offset — the caret leaves it, a selection appears or a
second caret is added — so after a click elsewhere Tab indents again; an edit moves the caret
too, and the recompute it schedules asks again for the new spot — or keeps the `kept` rest
Ctrl+→ left there (under `completion.rs`). **Except a move that typed the ghost's own next
letters**: before clearing, the effect hands the rope and the new caret to
`completion::typed_through` — and slices nothing itself, since the offset may no longer fall on
a character (under `completion.rs`) — and where that answers it publishes the rest at
the new caret through `ghost_at` as a `kept` ghost, `remembered` carried on — which the
recompute the keystroke schedules then keeps (`keeps_ghost`) instead of asking mid-word, where
neither predictor answers. It fires on the cursor change and reads the document untracked, so
it counts on the rope already holding the typed text when the caret moves; were floem to move
the caret first, the length check in `typed_through` fails and the ghost is cleared — the
behaviour before, so that order fails safe rather than keeping a rest for the wrong text.
It used to read `get_selection()`, which sees only the first region, and so could not judge a
cursor of several.
**Nor does a ghost outlive what it was made under**: a second effect, tracking `predictions`,
`remembered_predictions`, `active_conn`, `active_db`, `dialect` and the schema, clears it
whenever one of them changes. The setting
was read untracked, and only when a ghost was asked for, so switching *Inline predictions* off
— or the tab moving to another connection, database or engine — left the ghost drawn, and Tab
still took it, until the next edit asked again; *Predictions from past statements* is tracked
there for the same reason, and the schema because a ghost read from it — a join guessed through
a key since dropped — is stale once it refreshes. **The schema is `db_nodes` *and* every node's
inner `schema` signal**: a per-database Refresh, and the re-introspection after DDL, write the
inner signal and leave the list alone, so tracking the list alone would miss both. Nor does a
ghost outlive the editor's focus: the `editor_view_focus_lost` effect clears it, since a ghost
is an offer to the keyboard, an unfocused editor has none, and drawn there it read as text in
the buffer; the next edit after focus returns asks again. It clears on every blur, outside the
`blur_dismisses_completion` test that spares a press on the popup (the Floem gotcha on views
that must survive their own click) — which is safe only because no view is keyed on the ghost:
every read of `comp.ghost` is untracked, and `set_ghost` reaches the screen through `cache_rev`
alone. Like the cursor effect both live in the view and have no unit test.
`QueryPaneParams::predictions` and `remembered_predictions` carry the two settings in,
`QueryPaneParams::history` the run log and `QueryPaneParams::tabs` the open tabs a ghost
remembers from (under `completion.rs`); `tabs` is threaded from `lib.rs`'s editor-area
`dyn_container`, where it is in scope.
