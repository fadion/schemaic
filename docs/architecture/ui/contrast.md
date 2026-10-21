# `ui/contrast.rs`

`contrast.rs` — the **legibility gate** over `themes.rs`: WCAG relative-luminance maths plus
`UI_PAIRINGS`/`EDITOR_PAIRINGS`, one row per (foreground, background) combination the UI really
paints, each with the floor its role earns. The unit is the **pairing**, not the literal — a
grep for hardcoded hex can't see a paired accessor whose two halves are chosen in two different
views, which is how three sites shipped at 1.02:1 in *both* themes. Chrome that is below AA
today is listed in `UI_SHORTFALL` with the ratio it manages: an unlisted pairing must meet its
floor (so a new colour, surface or theme is held to AA), a listed one may never get worse, and
a listed one that now passes must be deleted — the baseline can only shrink. **A baseline entry
is keyed on `(theme, fg, bg, role)`, the role included**: keyed on the colours alone, *reusing*
a listed colour in a harder role was invisible to the gate, which is how every form caption in
the app came to be painted at 2.55:1 under a row baselined for icons. Adding a theme
needs no work here; painting a role on a new surface means adding its row.
One background the table **cannot** express is composed at run time: an ER-diagram card header
carries the table's identity colour washed over `erd_node_header`, and the `pair!(text on
erd_node_header, …)` row covers only the untinted case. So
`an_erd_header_tint_keeps_the_table_name_legible` asserts that pairing separately — `text` on
every `CONN_COLOR_PRESETS` entry at `erd_view::HEADER_TINT_ALPHA` over that surface, every
built-in UI theme, against the `Body` floor of 4.5:1, worst case 5.0:1 (Amber on Dark).
The colour set is closed and the wash strength is ours, so the promise can be kept and is
measured. A failure means the alpha is too high, not that a preset is wrong.
**The environment badge used to be excused from exactly that question** — "no theme can promise
a ratio on an arbitrary connection colour" — which was the opposite of what the clause beside it
argued about the same eight presets, and with *more* to go on rather than less: the connection
form's colour control is a row of `CONN_COLOR_PRESETS` swatches with no free-hex entry, and the
badge's fill carries no wash over it at all, so it is the easier half of the two. Fixed white
measured 1.75:1 on Amber — under even the *Recessive* floor of 2.0 — and under Body on eight of
eight, on the label whose whole job is to say you are on production. `theme::env_badge_text_on(fill)`
replaces it and decides **by measuring** rather than by a luminance threshold: a threshold is a
guess at where the crossover is, while `contrast_ratio` is the function every other colour here
is already held to and cannot be off by a preset. On the eight shipped it answers a near-black
for all eight (white's best is 3.82:1 on Red, the near-black's worst 5.02:1), and a darker ninth
preset would correctly get white with nothing touched;
`the_env_badge_label_is_legible_on_every_connection_preset` asserts that through the accessor
rather than a copy of it. The header reads fill and label in the same style closure, so a colour
switch moves both and neither is frozen at build time. `caption_close_hover`'s fixed white is
**not** the same case and no longer cites this one: one saturated red measuring 5.66:1 in every
theme is a different question from a palette of eight, three of them pale, and the shared
comment is what made the badge's exclusion read as considered. That test covers the **header only** — the same card's tinted border is deliberately not
asserted here, because a border carries no text and a legibility floor on it would mean nothing;
`erd_view::tests::a_tinted_border_is_never_fainter_than_the_plain_one` holds it to the plain
`border` it replaces instead.
**The tables cannot express a cross-axis pairing — and nothing paints one any more.** Every row
above holds one theme against itself, so neither table could see the combination two panels
once painted: the *editor* theme's token palette on a surface the **UI** theme chose. The
History and Snippet previews took their token colours from the editor theme and their
background from `bg_editor` — the UI theme's inset surface (the SQL editor's backdrop, the blob
viewer, the record editor's value fields; form fields default to `bg_deepest`, whatever the
table's first labels said) — and on the Light UI theme with the shipped-default Tokyo Night that put `string` at 1.70:1 and
`number` at 1.90:1, under even the *Recessive* floor of 2.0, while the uncoloured text beside
them sat at 5.70:1. The text the colour was added to make stand out was the only text made
unreadable. The first fix moved those previews, then the SQL `edit_field`s and the `GRANT`
block, onto the editor's own surface and base (`theme::preview_bg`/`preview_fg`) and gated them
with `a_coloured_preview_is_legible_in_every_ui_and_editor_theme_pair`, a walk of the whole
`(UiThemeKind, EditorThemeKind)` product. That made them legible and left them editor-coloured
blocks inside UI-themed forms and panels, so the second fix moved the palette instead: SQL
outside the editor takes the UI theme's own `sql_*` roles (under `sql_highlight.rs`), the
cross-axis test was deleted with nothing left crossing axes for it to measure, and those roles
are ordinary `UI_PAIRINGS` rows — `sql_keyword`, `sql_string`, `sql_number` and `sql_function`
at *Body* and `sql_comment` at *Icon*, each on `bg_deepest` (the SQL fields, the DDL preview's
box, the `GRANT` block) and on `bg_panel` (the History, snippet and AI-chat SQL), all passing
with no `UI_SHORTFALL` entry. A SQL field's placeholder is the field's own `theme::placeholder`,
as on any other field. Should a colour from one axis ever be painted on the other's surface
again, it needs a cross-axis test back: neither table can see it.
