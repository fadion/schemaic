# `ui/shortcuts.rs`

`shortcuts.rs` — the app's keyboard shortcuts as **one table** (`SHORTCUTS`), which
`settings::help_overlay` renders straight from — plus the tests that keep it honest. This list
is the app's *only* keyboard documentation and for Ctrl+H / Ctrl+G the only affordance of any
kind, so a binding missing from it is a feature nobody can find; it was a literal inside the
modal and drifted exactly as its own comment predicted, hiding Alt+↑/↓, Ctrl+↑/↓, Ctrl+Shift+C/V
and Ctrl+Home/End. **The handlers can't render from the table** — they are `match` arms on
`Key::Character` spread across the files `KEY_FILES` names — so the guarantee runs the other
way: the tests scan those files for the **five** idioms the codebase binds a
**Ctrl/Alt + letter** with (the `"x" | "X"` case pair, `eq_ignore_ascii_case`, `NavKeys`'
`Some("x") =>` *and* its `ch == Some("x")`, and `KeyCode::KeyX` for the physical match
Ctrl+Alt+L needs) and fail when one has no row, with `EXEMPT` the justified-baseline
escape hatch in the spirit of `contrast::UI_SHORTFALL`. **`EXEMPT` is `(file, letter, reason)`
and is scoped to one file, for the reason `KEY_FILES` is**: a crate-wide exemption goes blind
everywhere at once, so exempting Ctrl+Z for the masked secret field's sake would have vouched
for a real editor undo binding added later, in a different file, doing a different thing. Its
two entries are `lib.rs`'s `z` and `y` — a masked field's undo/redo, *suppressed* rather than
bound, because the document's history is a history of mask characters. The equality form is where **both**
Ctrl+Shift+letter bindings live and was missed; the gate was green only because `p`/`t` are
bound a second time in the unshifted arms. `KEY_FILES` pairs each file with the `SHORTCUTS`
**groups** its bindings may be documented in, because a table-wide lookup let the editor's
Ctrl+D vouch for a grid Ctrl+D — a different key doing a different thing. **It is
hand-maintained, and it was short rather than merely short-able**: the ER diagram binds Ctrl+F,
that binding is the find bar's *only* affordance (no toolbar button, no menu entry), and because
`erd_view.rs` was not on the array `bound_letters` never read the file — so the module whose doc
opens "a binding missing here is a feature nobody can find" passed green over exactly that,
beside a `SHORTCUTS` table with no ER-diagram group at all. Adding the row with the group still
missing turns both `every_bound_letter_is_documented` and
`every_key_file_names_real_shortcut_groups` red, which is also the demonstration that the gate
works once the file is in scope. The stronger, non-list-based form — walk `src_dir()` with an
EXEMPT list — is still not written. Deliberately
**weak**, like `doc_coverage`: it catches the binding nobody wrote down, not an inaccurate
row, and a *named* modified key (`Ctrl+Tab`, `Ctrl+Enter`, Ctrl+1‑9) is matched as a
`NamedKey` and so is outside the scan entirely. Two rules
earned their own tests — the scan skips `#[cfg(test)]` modules (a grid fixture's
`Some("b".to_string())` was reported as a phantom Ctrl+B, and a gate that cries wolf gets
deleted) and each idiom is pinned against synthetic input, since a scan that silently stops
matching still passes a test that looks for what's missing. Only **modified letters** are
gated: plain keys are bound in dozens of places for ordinary navigation, so gating them is all
noise, and they're what a user tries anyway.
`COMMAND_KEYS`/`command_keys` is the second consumer: the **command palette** shows a row's
binding as a keycap at its far right, which is where someone who can't remember a key actually
looks. Only where the command does *the same thing* the key does — a narrower set than it looks,
since `Run` runs all statements while Ctrl+Enter runs the one under the caret, `Terminal` and
`Ask AI` act on an argument where the keys only toggle a panel, and `Toggle Panel` names its
panel as an argument so it has three bindings and therefore none. A nearly-right keycap is worse
than none: it teaches a key that does something else. Each entry carries its row's **group**
as well as the key string, which must be **byte-identical** to a `SHORTCUTS` row in that group
(tested) — the table has two `Ctrl+G` rows meaning different things, so a string match alone
let either vouch for the keycap; the
other half — that each name still names a live command — can only be checked against a built
registry, so it rides `overlays::assert_names_match_labels`' `debug_assert`, without which a
renamed command would drop its keycap in silence. The name it checks is the command's `id`, its
English name, never the translated label, so the mapping holds in every language.
`MenuKey` is the third: the context menus' half of `COMMAND_KEYS`, held to the same rule (which
entries earn one is under *Popup menus*). It is an **enum rather than a string**, so a menu
cannot name an entry that is not there, and `MenuKey::row` names its `SHORTCUTS` row by group,
keys **and description** — stronger than `COMMAND_KEYS`' group-and-keys, so a row reworded
under it fails the test rather than going on vouching for the keycap. `keys()` hands it out through `keys_label`, so it is
respelled for the platform and the language like the rest. `every_menu_key_is_the_shortcut_row_it_names` checks each
variant, walking `menu_keys_all()`, which sits beside an exhaustive `_exhaustive` match so a
new variant fails to compile until it is listed — a list the test walks is only as good as its
completeness. Palette and menus draw one keycap, `widgets::keycap`.
**A group is an identity, its heading is text, and the two are kept apart for translation.**
`Group` is an enum (`Global`, `Editor`, `ResultsGrid`, `SchemaTree`, `AiPanel`, `Terminal`,
`ErDiagram`) whose `label()` is the `tr!` heading, because `KEY_FILES`, `COMMAND_KEYS` and
`MenuKey::row` all name groups for the tests above and none of that may change with the
language — a heading string would have made every one of those checks depend on the locale. A
row's description is a `Describe = fn() -> String`, a non-capturing closure over `tr!`, so
`SHORTCUTS` stays a `const` while each message id is still checked at compile time; the key
string beside it is never translated. So `MenuKey::label` answers a `String`, and
`every_menu_key_is_the_shortcut_row_it_names` compares rendered descriptions.
**The primary modifier is asked for, never spelled at the use site.** The modal said `Ctrl+…`
everywhere, and simply relabelling it for macOS would have been worse than the wrong word: the
grid and the terminal already took `.control() || .meta()`, but the editor pane, the ER diagram,
the window-root Global-nav dispatch and the AI panel's Ctrl+↑/↓ recall were `.control()`-only,
so a `Cmd+K` row would have documented a key that does nothing on most of the surfaces it names.
Both halves therefore hang off one capability — `PRIMARY_IS_CMD =
cfg!(target_os = "macos")`, the single `cfg!` for this concern, per *Ask the capability, never
`cfg!(target_os = …)` at the use site* (`core::window_chrome` above). What the modal **says**
goes through `keys_label`, called from `settings::shortcut_row` (the one choke point for every
row the modal draws) and from `command_keys`, so the palette's keycap is respelled the same way
— `PaletteItem::keys` is a `Cow<'static, str>` for exactly that. The tables stay Ctrl-spelled: a
second Cmd-spelled table would drift, and `every_command_key_is_a_real_shortcut` works by
comparing the two byte for byte, so respelling at the source would have dissolved that
guarantee. **The interface language is the respelling's second half, applied the same way** —
key by key on the way to the screen, never through a translated table, for the reason a Cmd
table was refused. `keys_label_on(keys, primary_is_cmd, name)` splits a row into bindings on
` / ` and each binding into keys on `+` — so the `/` in `Ctrl+/`, with no spaces around it, is a
key and not a separator — turns `Ctrl` into `Cmd` where Cmd is the primary modifier, and
otherwise asks `key_name`, which maps `Ctrl`, `Shift`, `Alt`, `Enter`, `Del`, `Home`, `End` and
`Space` to `key-*` catalogue messages. German reads its keyboard's legends (`Strg`, `Umschalt`,
`Eingabe`, `Entf`, `Pos1`, `Ende`, `Leertaste`), as its catalogue's own prose does (`Strg+→`
in the predictions hint). A key written the same everywhere (a letter, an arrow, `Tab`,
`Esc`, an F-key) has no message, and neither has `Cmd`, which is macOS's name in every language
and wins over the language's name for Ctrl (`cmd_is_named_before_the_language_is`). It
replaced a substring replace of `Ctrl` with `Cmd` that knew nothing of keys. English
stays byte-identical, and that is pinned rather than assumed:
`english_names_every_key_as_the_table_spells_it` runs every `SHORTCUTS` row through the real
namer and expects it back unchanged. Every caller — the modal, the palette, `MenuKey::keys`, the
tooltips, the new tab's prompt, the quick-fix hint — takes the language with no change of its
own. **A key label is respelled, never spelled, and that is a source gate** —
`no_view_code_spells_the_primary_modifier_in_a_label`, over both view crates, because the three
sites that broke it were `&'static str`s in view code that no runtime test reaches. What they
produced: on macOS the Shortcuts modal, the palette and the ER diagram all said `Cmd+Enter` while
the new tab's own prompt — the first thing on screen, and the only place the app says how to run
something — said `Ctrl+Enter`, as did the commit button's tooltip. The handlers were right either
way (`primary_held` is additive); what was wrong is that the app advertised two keys for one
action. The needle is `Ctrl+` **inside a string literal**, so `Ctrl` alone in prose or code is not
it, and two shapes are recognised as not-labels rather than exempted by file: an argument to
`keys_label` itself, which is a Ctrl-spelled table entry on its way *through* the respeller, and
the trailing description of a `pair!`/`faded!` row in the contrast audit, which names a colour
pair for a report and never reaches a view. What the handlers **accept** goes through
`primary_held`, which is *additive* — Ctrl
everywhere, Cmd as well on macOS, so the Ctrl bindings a macOS user already learned keep
working. Off macOS it ignores Meta deliberately, because there it is the Windows/Super key and
answering to it would take over shortcuts that belong to the desktop
(`cmd_counts_as_the_primary_modifier_only_where_cmd_is_the_modifier` pins that direction).
Both pure halves take the capability as an argument (`keys_label_on`, `primary_from`; the first
takes the namer too, so German is testable without switching the catalogue) so either
platform is testable from whichever one runs the suite — and that is also what makes the blast
radius provable rather than argued: `primary_from(c, m, false) == c`, so off macOS the whole
change cannot behave differently, and on macOS it acts on keys that previously did nothing.
**The negative checks are the ones to be careful with.** `!primary_held(mods)` is how the editor
pane asks *is this plain typing* — the Tab indent, the auto-pair block and the `ctrl`
argument it hands `completion::types_a_character` — so one left as `!mods.control()` would let a
Cmd chord both fire a shortcut and insert a character. Two surfaces stay hand-spelled on
purpose: the terminal's `encode_key` and its Ctrl+Shift+C/V, and the grid's two
`control() || meta()` reads. They answer as `primary_held` does on macOS and a shade wider
elsewhere, since they also take Super; and `encode_key`'s `!mods.control()` on the Alt branch is
byte encoding for AltGr rather than an app shortcut, so converting that one would be a bug.
**The modifier going down or up on its own is a key, not a state**: `is_primary_key` (Control;
Meta only where `PRIMARY_IS_CMD`, through the testable `primary_key_from`) asks about the key,
since a lone Ctrl's own key-down does not reliably carry Ctrl in its modifiers yet.
`modifier_keys()` is the window-wide answer — an `RwSignal<HeldModifiers>` (the primary
modifier, Shift, Alt) in a detached-scope `thread_local`, the `widgets::window_size` shape —
written by `note_modifier_key` only on a change, because a held key repeats its key-down, and
corrected by `note_pointer_modifiers` from a pointer's modifiers. It exists for the editor's
Ctrl+hover underline (`editor_pane.rs`), and is a signal of its own because floem never hands a
key to an ancestor of the focused view (*Floem 0.2 gotchas*). **`opens_link(primary, shift,
alt)` is the one Ctrl+click rule** — the primary modifier alone — which the click, the pointer
move's underline and the key-driven underline all ask: the last asked Ctrl's own key alone, so
Shift-then-Ctrl (how Ctrl+Shift+F starts) drew the link line on a word a Ctrl+Shift click does
not open (`shift_or_alt_then_ctrl_opens_no_link`). `primary_key_tests` pin both platforms.
