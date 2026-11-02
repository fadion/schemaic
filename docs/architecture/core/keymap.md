# `core/keymap.rs`

`keymap.rs` — **the rebindable shortcuts**: which commands a key can be bound to, the key each
has by default, what the user changed, and which command a pressed chord runs. Pure over values:
the window-wide signal, the file I/O, the list of fixed keys and the reading of a key press are
`ui/shortcuts.rs`'s, and the modal that edits it is `ui/shortcuts_view.rs`. **It is one table**,
and that is the point of it: every handler that runs one of these commands asks
`Keymap::resolve`, and the Shortcuts modal, the palette's keycaps and the editor menu's *Format
code* show `Keymap::chord`. A handler with its own `match` arm for a command's key would go on
answering the default after the user moved it, while the modal documented a key that no longer
did what it said — which is why the editor's copy of the global keys, once a second set of hand
matches, now asks this table too.
**`Command` is seventeen variants**: Find anywhere, the command palette, new / close / next /
previous / reopen tab, open / save / save-as file and the three panel toggles, all
`Context::Global`, and Format code, Format statement, Make single line and Go to line,
`Context::Editor`. Its `id()` (`find-anywhere`, `toggle-terminal`, …) is what `keymap.json`
stores, so **an id is identity: add a command, never rename one** — a renamed id turns every
saved binding of it into an unknown command on the next start. `ALL` is the modal's order and the
tie-break order below; `all_lists_every_variant` holds it to an exhaustive `match`, so a new
variant fails to compile until it is listed. What is *not* a command stays fixed: grid cell
movement, editing inside the editor, Tab, Enter, Escape, terminal input, and Ctrl+1…9, a family
of nine rather than one command to move. The caller passes the fixed keys it binds as
`FixedKey { context, chord }`, so a command is never put on one — which would leave either the
command or the fixed key dead.
**`Context::overlaps` is the whole conflict rule.** A `Global` key is heard whatever has focus,
so it overlaps every surface; two surfaces never overlap each other, which is how the editor's
Ctrl+G (Go to line) and the grid's Ctrl+G (Go to row) mean two things — and why a Global
command cannot take the editor's fixed Ctrl+F while Go to line may take the grid's F6
(`a_fixed_key_is_refused_where_it_is_heard_and_free_elsewhere`;
`a_global_command_cannot_take_a_surfaces_fixed_key` over the real fixed list, in `shortcuts.rs`).
**A `Chord` is `{ primary, alt, shift, key }`, written in the Shortcuts table's own spelling** —
`Ctrl+Alt+Shift+L`, the modifiers always in that order whatever order they were read in — so a
written chord goes through the modal's respeller (`Ctrl` → `Cmd`, `Del` → `Entf`) unchanged.
`primary` is written `Ctrl` and read as Cmd on macOS, so one `keymap.json` serves every platform.
A character is held lower-case and unshifted (`Ctrl+Shift+P` is `Char('p')` with `shift`), so a
chord does not depend on what Shift makes of a key on one layout. `parse` reads `Cmd`, `Command`
and `Control` as `Ctrl`, `Option` as `Alt`, a hand-written key name (`Return`, `PageDown`, `Up`)
as the table's, and `Ctrl++` as the plus key; it refuses an empty string, modifiers with no key,
a part that is no key and a modifier written twice, each its own `ChordError`
(`a_chord_reads_back_as_the_table_writes_it`, `what_is_not_a_chord_is_refused_with_its_reason`).
**`Chord::refusal` is what no command may have, whatever else is bound.** A character or Tab needs
the primary modifier: without it the key types or moves focus, and Alt alone is how macOS types
an accent (Option+E) and many layouts type `@` or `{`. Any other named key needs Ctrl or Alt; a
function key needs neither. Escape is never a key, Ctrl+Esc included — it closes what is open
everywhere and is the capture's own cancel. The editing keys every text field answers are
reserved, since a command on one would take it from every field in the app: Ctrl+C/V/X/A/Z/Y,
Ctrl+Shift+Z, and Ctrl, shifted or not, with an arrow, Home, End, Backspace or Delete. Shifted,
only Ctrl+Shift+Z is a field's — Ctrl+Shift+A is the AI panel's default — and with Alt as well
none of them is. Alt+F4 is the desktop's. Every default passes
(`every_default_is_a_chord_a_command_may_have`, `the_editing_keys_every_field_answers_are_reserved`).
**`keymap.json` (`FILE`; `KeymapFile { bindings }`) holds only what the user changed**, by command
id, so a command they never touched follows its default and a default changed in a later release
still reaches them. A cleared command is `null`, which is a different thing from absent
(`the_file_holds_only_what_moved_and_a_cleared_command_as_null`); a command moved back to its
default leaves the file, and an untouched keymap writes an empty one.
**`Keymap::load(file, fixed)` reports every entry it cannot apply — and keeps it.** An unknown id,
an unreadable chord, a refused one and one on a fixed key each become a `Problem`, and that
command runs the session on its default; the entry itself is held in `Keymap`'s private `kept` and
goes back out on the next save, so a newer build's command, or a typo the user means to fix, is
not erased by a save from this one
(`every_entry_that_cannot_be_kept_is_reported_and_kept_in_the_file`). **What the file names wins
over a default**: a file moving New tab to Ctrl+P takes Ctrl+P from Find anywhere, reported as a
`Conflict`, rather than leaving New tab unbound
(`what_the_file_names_wins_over_a_default_and_the_loss_is_reported`); between two entries of the
file, the first in `Command::ALL` order keeps the chord
(`between_two_entries_of_the_file_the_first_command_keeps_the_key`); a cleared default frees its
key for another entry. Every loser is a `Problem` — nothing is resolved silently — and the UI
turns the list into a notice.
**The modal's questions are `check(command, chord, fixed) -> Verdict`**: `Refused` first, before
anything is looked up; then `Unchanged`; then `Fixed(i)`, an index into the caller's list, which
knows what that key does; then `Taken(holder)` for a command heard where this one would be; else
`Free`. `assign` is the *Replace* of a `Taken` — it takes the chord from its holder — and refuses
a fixed or refused chord outright (`a_fixed_or_refused_key_is_never_assigned`). `reset` is
`assign` of the default, so a default since given to another command is taken back from it
(`resetting_onto_a_key_since_given_away_takes_it_back`), and `check_reset` is the question before
it. `resolve(chord, context)` matches the context exactly, so the editor — which consumes every
key — asks for `Global` first and then for its own. `changes_from` counts the commands two keymaps
bind differently, the figure a settings import names: taking Ctrl+P moves two commands, so it
counts two (`changes_count_the_commands_bound_differently`).
**Not here, by decision:** two-step chords (Ctrl+K Ctrl+C).
