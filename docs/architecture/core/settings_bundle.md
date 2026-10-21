# `core/settings_bundle.rs`

`settings_bundle.rs` — **the settings file**: the user's preferences and snippets in one JSON
file, to take to another machine or hand to a colleague, and read back. Pure over text and
values — the app gathers the current `UiState` and snippets, picks the file and applies what
comes back (Settings → General's *Settings file* row and the palette's *Export Settings* /
*Import Settings*; the app's two trips are described under `settings.rs`'s `bundle_row`).
`export_json(state, terminal, snippets)` writes the envelope
`{schemaic_settings, settings, terminal, snippets}`, pretty-printed, at `BUNDLE_VERSION` (1) —
`terminal` is any `Serialize`, because the terminal's settings type is
`schemaic_term::TerminalSettings` and this crate does not depend on that one, so the app passes
its own and the tests a mirror of its shape. `parse` reads it back BOM-tolerant, refuses a file
without the `schemaic_settings` key and one at a later version as *written by a newer
Schemaic* rather than half-reading it, and drops a snippet it cannot read alone rather than
the whole file. `BUNDLE_FILE_SUFFIX` (`schemaic-settings.json`) is only the save dialog's
default name: unlike the connection export's suffix, nothing recognises a bundle by it — the
import's picker has no filter, and the envelope key is what `parse` checks.
**`PORTABLE_KEYS` is an allowlist of `UiState` field names, and that it is an allowlist is the
point**: a field added to `UiState` later stays home until somebody decides it travels, where a
denylist would export it the day it lands. What travels is the themes and scale, the interface
language (`ui_language` — refused, through the app's `normalise`, when it names a language this
build does not offer, since `LanguageChoice::from_key` reads that as `system`), the editor,
formatting, the row limit and statement timeout, `confirm_writes`, `live_validate`,
`notify_finished`, `restore_tabs` and the assistant's effort, instructions, schema scope and
gutter. What stays is the panel widths, the history panel's *Current | All* switch
(`history_all_connections`) and the schema filter and visibility (this window's),
the expanded and hidden sets and the activity intervals (keyed by connection ids, which differ
per install), `ai_harness`, `ai_model` and `ai_cli_path` (they name what is installed *here*),
and the legacy `ai_run_queries` consent flag; `write_log_values` stays too, because a file from
someone else must not be what turns on keeping the write log's values in the clear — and
nothing tests its absence, the allowlist being all that keeps it home; `extra` and
`show_table_sizes` are not on the list
either, and nothing records why for the second. `parse` keeps only listed keys, so a
hand-edited file cannot slip in a CLI path (`a_key_outside_the_allowlist_is_never_applied`),
and `every_portable_key_is_a_field_ui_state_writes` fails on a name that is not a field — a
renamed field would otherwise silently stop travelling.
**`apply(settings, state)` tries each key alone through `UiState`'s own serde**, so a value is
accepted exactly when a `ui_state.json` holding it would be read, and one that would not leaves
that field as it was without spoiling the rest. It answers the new `UiState` and how many
fields *actually changed* — compared after the round trip, since a shim may normalise a value
onto the one already held (`applying_counts_only_what_changed_and_skips_an_unreadable_value`).
**"Readable" is the app's word too, through `apply_with`**: the themes, the scale, the effort
and the schema scope are plain strings in `UiState`, and the font size and tab width bare
numbers, so serde alone would let a theme key this build does not know, or an out-of-range
font, through — to be counted as a change and then land as the app's fallback or clamp. So
`apply_with` takes the app's own reading as `normalise` and refuses a value that reading would
rewrite (`a_value_the_apps_own_reading_would_change_is_refused`); `apply` is `apply_with` with
nothing to normalise. Both are now thin: the rule itself is `apply_keys(keys, settings, state,
normalise)`, generic over any `Serialize + DeserializeOwned + Clone` settings type, and
`apply_with` is `apply_keys(PORTABLE_KEYS, …)` over `UiState` — the generic form exists so the
terminal's half goes through the same per-key, serde-then-`normalise` test rather than a second
copy of it.
**The terminal's preferences travel under their own allowlist, `TERMINAL_KEYS`** — `font_size`,
`copy_on_select`, `cursor_style` and `cursor_blink` from `terminal.json` — and **never `shell`**,
which names a program installed on this machine: a file from someone else must not choose what
the terminal launches. `Bundle::terminal` is that section as `parse` filtered it, so a shell
written into a hand-edited file is dropped on reading, and `apply_keys` checks the allowlist again
when handed one anyway (`a_shell_named_in_a_bundle_is_never_applied`; the export side is
`the_terminal_travels_without_its_shell`). The terminal half is refused by its own reading just as
the settings half is (`the_terminal_half_refuses_what_its_own_reading_would_change`). A bundle
written before the terminal travelled has no `terminal` section and reads as an empty map —
`BUNDLE_VERSION` stayed 1 (`a_bundle_from_before_the_terminal_travelled_reads_with_none`), and in
the other direction `parse` only ever looks up the sections it knows, so a build from before
reads a newer file's `terminal` as nothing rather than refusing the file.
**A bundle never lowers a guard**: `confirm_writes` is taken only when it turns confirmation
*on*, so a file from someone else cannot quietly remove the question before a write
(`a_bundle_can_turn_confirmation_on_but_never_off`).
**Snippets travel when they are the user's own and scoped `Global` or `Dialect`** — a
`Scope::Conn` names a connection id that means nothing on another install, and a built-in ships
in the code anyway. `merge_snippets(existing, incoming)` adds only travelling ones, skips one
whose name *and* body a snippet already has, drops an abbreviation already taken
(case-insensitively, as `by_abbrev` matches) rather than leave two snippets answering to one
spelling, and gives each a fresh id through `snippet::next_id` one at a time — so two incoming
snippets never take the same id — with `last_used` cleared and `source` `User`
(`merged_snippets_get_fresh_ids_and_never_collide`). `summary(changed, snippets)` is the
phrase the confirmation names after *changes* — *3 settings and adds 1 snippet*.
