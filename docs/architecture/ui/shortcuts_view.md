# `ui/shortcuts_view.rs`

`shortcuts_view.rs` — **the Shortcuts modal**, opened from the header's help (?) glyph
(`LayoutUi::help_open`), and the one place a command's key is changed. It moved out of
`settings.rs` with the keymap — `help_overlay(LayoutUi)`, `shortcut_row` and `shortcut_group`
came with it, and what the `settings.rs` entry says about the body's scroll cap, the wrapping
description and the catalogue still holds here — and it owns only the layout and the editing
gesture: the rows are `shortcuts.rs`'s, the keymap's commands, whose keys the user can move, and
the fixed `SHORTCUTS`, which they cannot. Each group shows its commands first (`Group::of`, by
the command's `Context`), then its fixed rows. Mounted beside Settings in the modal layer's
settings group, raised by `settings_modals_up`'s `help_open` arm, and on
`modal_backdrop_gate::PAINTS_A_BACKDROP`. The body is `autohide(scroll(…))` under
`modal_body_h(520)`, in a `modal_w(552)` panel.
**A command row is a focus-ring row and a fixed row is not** — there is nothing to do on a fixed
one. Command rows join the ring through `in_focus_ring_unless` at `VALUE_TAB` plus `ROW_TAB_STRIDE` per
row in modal order, after the search box at 10. A row's menu — right-click, Shift+F10 or the
ContextMenu key — is *Assign*, *Clear* (disabled when it has no key) and *Reset to default*
(disabled on the default), raised through the published `widgets::menu_channel()` with
`set_menu_return` armed to the row, so the keyboard comes back to it when the menu goes, as a
tree row's does (`schema_tree::arm_menu_return`). Double-click or Enter assigns as well. The
menu's *Assign* starts the capture **a tick later** (`exec_after(ZERO)`): run from the entry, its
focus request landed while the panel still held the keyboard, and the capture never took.
**Every row keeps the same horizontal inset** (`row_inset`: padding out, margin back by the same
amount, and no `width_full` beside it — a full width plus a negative margin moves the box rather
than widening it, which left a command row's key 12px short of a fixed row's), so a command
row's hover fill overhangs evenly and its text lines up with a fixed row's. **A key slot keeps its
height** whatever it holds: the prompt and *Not set* stand beside a zero-width keycap
(`key_slot_text`), so a row does not shrink and pull the rows under it up.
**Assign makes the row the capture.** It sets `Edit::Capturing` and focuses the row, whose own
`KeyDown` listener reads the next press through `shortcuts::chord_of` and asks `Keymap::check`.
The verdict's step is `step_for`'s, pure and tested. `Free` and **`Unchanged`** both assign and
save at once — choosing the key a command already runs on is an answer, and `Keymap::assign` is
what drops a kept entry the file could not apply; ending quietly on `Unchanged` left a kept
`Ctrl+Banana` in the file and its startup notice coming back
(`assigning_a_kept_commands_own_key_clears_the_kept_entry`), and *Reset to default* is offered
while `Keymap::keeps` the command's entry, not only off the default (`reset_offered`). `Taken` puts *Replace* / *Cancel*
under the row — text buttons in the manual transaction's Commit and Rollback colours
(`theme::tx_commit`, `tx_rollback`), outside the ring, since Enter and Escape on the row answer
them too — so nothing is taken from another command
without asking; a fixed or refused chord writes its reason under the row — `keymap-fixed`
naming what the fixed key does, `keymap-refused` with `shortcuts::refusal_text` — and **the row
goes on listening**, so the user tries another key rather than starting over. A lone modifier is
no chord and is waited past; a plain Escape cancels. *Reset to default* goes through the same
path with the default chord, so a default since given to another command asks before it is taken
back. Every change is `shortcuts::save_keymap`, the window's signal and `keymap.json` together.
**The ring stands down while the row takes the press**, and listener order is not what does it:
floem runs *every* `KeyDown` listener on a view and ORs their answers (`ViewId::apply_event`, and
the `fold` in `EventCx` — the hazard the results-toolbar bullet in *Floem 0.2 gotchas* names), so
the capture's `Stop` alone left the ring's own listener free to step a plain Tab off the row
(which went on showing `keymap-press`) and to blur it on the Escape that cancelled. The row is
wrapped in `widgets::in_focus_ring_unless`, whose guard is a per-press flag the row's listener
sets when it answers — *this press*, not "is it capturing", because the Escape that ends a
capture has already put the state back to idle by the time the ring's listener asks. The `Stop`
still keeps the press from the window root's handler and its `NavKeys`. **A click anywhere but
the capturing row ends the capture** — the panel's `PointerDown`, which hears a press after the
row it landed on (pointer events, unlike keys, do reach ancestors), so each row marks its own
(`RowCtx::pressed`) and the capturing one is spared. Not *focus loss*: that ended it too, and
the menu's teardown moving the keyboard was enough to cancel an *Assign* before it began. A
pending *Replace* is not ended by a click, since clicking its buttons is how it is answered.
**The search box** (`edit_field`, clearable) filters on what a row does or on its keys as shown,
case-insensitively, and on the keys with their `+`s and spaces dropped as well, so `ctrlt` finds
Ctrl+T (`row_matches`; `keys_are_found_without_their_plus_signs`); nothing matching is
`keymap-no-match`. **The body rebuilds on the query and never on a key change**: each command
row follows the keymap signal itself — its key is a `dyn_container` on `(chord, edit)` — so
assigning a key leaves the list, and the keyboard on its row, where they were. A query change
while a row is being changed returns it to `Idle`, since a row the search hides is a change with
nowhere to finish.
