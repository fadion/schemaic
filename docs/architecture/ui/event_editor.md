# `ui/event_editor.rs`

`event_editor.rs` — the **scheduled event** modal, over `core::ddl`'s `EventDraft`, on the
one engine that has them. Reached from the schema tree's Events folder (row **Edit**),
from the database and namespace **Create ▸ Event** submenus, and from Find-Anywhere.
Modelled directly on `routine_editor` — the same chrome, the same
seed-local-signals-then-write-back rule, the same `DdlUi::event_body` arrangement for the
field a late `SHOW CREATE` has to correct in place, the same `overlay_open_key` memo so that
correction doesn't tear the modal down, and the same `ddl_preview` ending. What differs are
four consequences of `ALTER EVENT` being in-place: there is no drop-and-create, so a rename
can't destroy the original and the footer's clash message says only that the name is taken;
the three Preview refusals (`event_source_pending`, `event_body_stale`, `clashes`) cost a
*rejected statement* rather than a lost object, and are still made because an error after
Apply is a worse way to learn this than a footer; **the schedule is the one part of the form
that rebuilds itself**, keyed on the shape (`EVERY` vs `AT`) alone and never on the draft, with
the draft written *before* the flag is flipped so the rebuilt fields seed from the shape being
switched to; and every timestamp field holds SQL, so it is monospaced and its placeholder
shows the quotes. The Status dropdown offers Enabled and Disabled plus, for an event already in
it, the replica state — offering `DISABLE ON SLAVE` freely would be offering a keyword MySQL
8.4 has removed, while hiding it would show "Enabled" over an event that isn't.
**A dropdown that offers "the standard list, plus whatever this event already uses" has to read
the second half from what the *server* reported**, not from the draft: `status_choices` and
`interval_units` both take `ui.ddl.event`'s `target.current` alongside the draft value, because
the form is torn down and rebuilt whenever the preview opens or closes. Seeded from the draft
alone, moving a `SLAVESIDE_DISABLED` event to Enabled, editing its body, pressing Preview and
coming back left the list reading `Enabled / Disabled` — "Disabled on replica" gone, no other
producer of `EventStatus` in the view, and the only way back to the state the event is actually
in being Cancel and reopen, which throws away every edit made in the same sitting since `open`
replaces the draft wholesale. `interval_units` is fixed with it rather than left as the one found
next time (`EVENT_INTERVAL_UNITS` is exactly MySQL's documented fifteen, so nothing is reachable
through it today), and it compares case-insensitively — the server's spelling is its own, and a
duplicate differing only in case is a list with two identical-looking rows.
**That state has two spellings and `EventStatus` keeps them apart** (`SlavesideDisabled` /
`ReplicaDisabled`), because the word the server reported in
`information_schema.EVENTS.STATUS` is the only signal for which keyword it will accept back:
8.4 renamed the column value *and* removed the old keyword. Folded together — as they were —
every statement used the pre-8.4 spelling, so Copy CREATE on 8.4 produced SQL it rejects and a
whole-database script stopped at that event. Two variants rather than one carrying a word, so
that equality still means "the same state" and the differ can't see an `ALTER` in an event
nobody edited.
It shares one tuple element with the trigger and routine overlays (Floem's 16-arity
`ViewTuple` limit) and clears both of their targets on open, as they clear its.
**Its words are the catalogue's** (`event-editor-*`) — sections, fields, the
schedule picker, the preserve toggle and its hint, the titles, the source-reading and stale
notes — with Cancel, Preview SQL, the change count and the comment placeholder the shared
`dialog-cancel`, `editor-preview-sql`, `editor-change-count` and `editor-comment-placeholder`.
**Both pickers match a pick back by the text they show**, so both sides of that comparison have
to be in the interface's language: the schedule's two entries were the consts `SCHED_EVERY`/
`SCHED_AT` and are now `sched_every()`/`sched_at()`, called once per build and compared against
that one value, and the statuses are `event_status_label(EventStatus)`, `EventStatus::label`'s
words drawn here (the core-words rule under `i18n.rs`), which the Status picker's `find` reads
the pick back through — `english_names_every_event_status_as_core_does` holds English equal to
core's. The local `bound_toggle` takes `String`s. The identifier and SQL placeholders
(`nightly_purge`, the `DELETE` example, `root@localhost`, the timestamp literal, `DAY`) stay.
The footer's refusals and Preview's readiness read `EventDraft::problems` and `clashes`, not
their English wrappers `validate` and `name_clash`: `event_problem_text` words each problem
from `event-problem-*`, and `event_clash_text` the clash from `event-name-clash`, `$name` the
draft's trimmed name (the editors' refusals, under `i18n.rs`) — both held by
`english_says_every_event_problem_as_core_does`, the clash against `name_clash` itself.
