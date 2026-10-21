# `core/locale.rs`

`locale.rs` — **which language the interface is drawn in**, decided once at launch. It answers
a question only the GUI asks: the CLI, the MCP server and everything sent to an AI harness stay
English, because scripts parse the first and a model reads the second, so nothing in core
renders through it (`ui/i18n.rs` is the half that does; the conventions are under *UI
conventions*, *The string catalogue*). `UiLanguage` has one variant per catalogue, `EnGb` the
source, then `De`, `Sq`, `It`, `Nl`, `Fr`, `Es`, `Ro` and `Bg` — every one complete, and every
one on `SHIPPED` (below) — and `UiLanguage::ALL` lists every variant — pinned by `all_names_every_language_once`,
whose exhaustive `match` makes a new variant a compile error until it joins the list, so the
tests that iterate `ALL` cannot quietly skip one. `catalogue()` is the directory its `.ftl`
lives in and `key()` — the persisted spelling and what `SCHEMAIC_LOCALE` takes — is simply
`catalogue()`, every language having a catalogue of its own; and
`endonym()` the language's name **in itself**, which is deliberately not in any catalogue: a
picker lists each language the same way in every language, so a reader lost in the wrong one
can still find their own. `from_key` reads the primary subtag case-insensitively, so `de-AT`,
`de_AT` and POSIX's `de_AT.UTF-8` are all German, `fr_CA.UTF-8` is French, `es-MX` Spanish
and `ro-MD` Romanian — one catalogue a language, the region dropped — and `en-US` is the one
English there is (`keys_round_trip_and_tolerate_case_and_region`; the tests' language nobody
knows is `ja-JP`, which was `fr-FR` until French had a catalogue). The two pseudo-locales that once stood
beside the real ones — `pseudo` and the doubled `pseudo-en`, English transformed to find boxes
too narrow for longer text — were removed once real translations could do that job, and
`from_key` now reads either key as one it does not know.
`number_marks()` is the language's decimal and grouping marks, a table rather than CLDR
(`german_swaps_both_marks_and_english_keeps_core_s`,
`each_translation_writes_its_own_group_mark_after_a_decimal_comma`; *Figures in the
interface's marks*, under *UI conventions*, has the table).
**`resolve(env, setting, os)` takes three sources in order, and only the first is
unconditional.** `SCHEMAIC_LOCALE` is a developer's, so it reaches whatever it names — a
catalogue still being written, when there is one, and today any language regardless of the
setting and the OS — and one it does not know falls through
rather than failing the launch (`the_environment_wins_and_reaches_unshipped_catalogues`). The
*Language* setting and then the OS's preferred languages, in preference order with an unknown
one passed over, reach only **`SHIPPED`** catalogues, and nothing matching is `EnGb`: a language
whose catalogue is half written would otherwise draw a half-translated interface for everyone
whose system is set to it, which is worse than English. A language joins `SHIPPED` when its
catalogue is complete, not when it is started, and complete is not enough on its own: shipping
is the user's call. Today it holds all nine, English first since Settings lists them in its
order — the eight translations shipped machine-drafted and unreviewed, on the user's decision
(2026-10-21) — and `every_catalogue_ships` pins exactly that: every `ALL` member on it, the
same length, `EnGb` at the head. A language added later starts off the list, so that test is
the one to change for it. With nothing unshipped left to point at,
`the_system_never_reaches_an_unshipped_catalogue` holds the property against `resolve_in` with
an English-only list — a German system and a `de` setting both get English there — and
`a_system_set_to_a_shipped_language_gets_it` checks the public `resolve` the other way: a
`fr-CA` system gets French, a `bg` setting Bulgarian. A
settings file that still names a retired pseudo-locale reads as the system's choice, and the
variable naming one falls through like any other unknown key
(`a_retired_pseudo_locale_key_is_the_system_s_choice`). The private
`resolve_in` takes the shipped list as a parameter so the ordering can be tested against a list
other than the one that ships — English and German only, so a known but unshipped French is
passed over (`the_system_is_read_in_preference_order`,
`a_shipped_setting_overrides_the_system_either_way`). `SYSTEM_KEY` (`system`) is
the setting's "follow the operating system", and `UiState::ui_language`'s default; that field is
a `String`, not the enum, so a key a newer build wrote reads as the system's choice instead of
failing the whole `ui_state.json`.
