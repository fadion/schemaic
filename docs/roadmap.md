# Schemaic — roadmap

The larger pieces of work that are planned but not started, each with the decisions already taken,
what is deliberately out of scope, and the order it should land in — several sessions, a design
choice or two, traps worth writing down before anyone walks into them.

Keep it disjoint from its neighbours: a fact about the system as it *is* belongs in
`architecture.md`, an instruction about how to work belongs in `AGENTS.md`. When a phase here
lands, its facts move into `architecture.md` in the same change and the phase is deleted from this
file rather than ticked off.

## Interface translation

Translate the GUI into languages other than English. Nothing exists yet: no localisation crate, no
strings table, every user-facing string an English literal at its call site.

### Decided

- **Fluent, through `i18n-embed` and `i18n-embed-fl`.** The `fl!` macro checks a message id and its
  argument names at compile time, so a missing key is a build error rather than a blank label.
  Fluent carries CLDR plural rules and whole-sentence messages a translator can reorder, which the
  current `text::plural(n, one, many)` and sentence-from-fragments style cannot express.
- **The GUI only.** The CLI (`schemaic query`/`exec` and siblings), the MCP server's tool
  descriptions and results, and everything sent to an AI harness stay English. Their output is
  parsed by scripts or read by a model, and a translated `schemaic exec` report would break the
  first and gain nothing with the second. Text shared between the GUI and one of those surfaces
  (`cli_install::report_*`, the export and compare notes) has to be split at that seam, not
  translated through it.
- **Server and driver errors stay in the server's language.** A translated sentence wrapping an
  English (or German, or whatever the server is set to) error is expected, not a defect.
- **British English is the base locale, `en-GB`.** The spelling rule in `AGENTS.md` becomes the
  source catalogue; an `en-US` catalogue is then possible but not planned.
- **No right-to-left interface in this phase.** Floem 0.2 has no layout direction, no start/end
  insets and no `text-align`; taffy 0.4 does not mirror; the UI hard-codes left and right in a
  few hundred style calls. Mirroring it is a separate project, if ever. Right-to-left *data* is a
  different matter — see below.
- **No Chinese, Japanese or Korean interface in this phase.** A romanised one is not an option —
  pinyin without characters is ambiguous and a romaji or romanised-Korean UI is essentially unseen
  — so the choice is a proper translation or none, and a proper one waits on the font question
  below. Many developers in those markets run technical tools in English anyway. CJK *data* is in
  scope regardless; see below.
- **German first.** It is a large developer market and runs longer than any other common
  interface language, so it is also the layout stress test phase 6 needs. The first catalogue is
  machine-drafted and ships unreviewed; proofreading waits until the project has users, who can
  then contribute corrections. Treat that as accepted, not as a blocker.
- **A language change applies on the next start.** The Settings override is read once, at launch;
  no label re-renders when it changes, which spares two to three thousand call sites a reactive
  dependency for a setting changed once. Changing it says so where the user changed it:
  *The language change takes effect the next time Schemaic starts.*

### Never translated

SQL of any kind (generated DDL, dumps, exports, formatted queries); SQL keywords and the plan
modal's *Analyze*; persisted names (serde keys, `favorites.json` and the other file names);
`pg_builtins.rs` and `mssql_builtins.rs`, which quote the servers' own text; CLI, MCP and AI text
as above; key names in shortcut labels (`Ctrl`, `Shift`, `Tab`) unless a platform convention says
otherwise.

### Phases

Each phase leaves English output byte-identical, so the existing suite — several hundred
assertions pin English wording — stays the regression net throughout.

1. **Infrastructure.** Add the crates, an `en-GB` catalogue, a loader chosen from the OS locale
   with a Settings override, and a **pseudo-locale** (accented letters, ~40% padding, bracketed)
   for a run that shows every clipped box and every string nobody extracted at once. There is no
   screenshot harness, so this is the layout check.
2. **Labels that are identities.** Before anything is translated, every place that compares
   displayed English to decide behaviour becomes an id: `sql::NO_DATABASE_SELECTED`, which the
   front ends recognise by its text; the menu code that matches on `"Favourite" | "Unfavourite" |
   "Colour" | "Hide"` in `overlays.rs`; `text::human_count`'s `k`/`m`/`b`, which
   `model::goto_row_index` parses back.
3. **UI literals** in `schemaic-ui` and `schemaic-app` — on the order of two to three thousand,
   heaviest in `widgets.rs`, `overlays.rs`, `lib.rs`, `grid.rs` and `shortcuts.rs`. Mechanical,
   one file per session, by hand: the rule against scripted edits applies in full. Retarget
   `source_gate::no_menu_label_ends_in_an_ellipsis` at the catalogue as labels leave the source —
   it asserts a minimum count of literal labels scanned and will fail as they go.
4. **Messages built in `schemaic-core`.** The hard part. Around 130 functions return finished
   English prose (`run_verdict`, the DDL preview notes, `Rollback::note`, `tx`, `dump`, `compare`,
   `stats`, `import`, `conn_import`, `users`), and core cannot — and should not — reach a UI
   catalogue. Each becomes a typed message (an id plus arguments, or an enum per area) the UI
   renders; core keeps an English rendering for the CLI and MCP. Settle the message type on one
   area first and review it before the rest follow. The existing enums are the nearest starting
   points, though they render in three different ways: `AliasRenameRefusal` through `message()`,
   `ImportError` and `ParamError` through `Display`, and `FilterError::BadCondition` not at all
   — it carries a finished `String` (the parser's text, or a sentence built where it is raised),
   so its prose lives at its construction sites and it is the weakest of the four to start from.
5. **Plurals and assembled sentences** — a few hundred sites using `plural(` or `== 1 { "" }`,
   folded into whole Fluent messages as their phase-3 or phase-4 file is converted rather than as
   a pass of their own.
6. **Layout for longer text.** German, Finnish and Russian run 30–100% longer than English.
   Roughly sixty fixed-width boxes sit beside text — the connection form, the import and compare
   panes, the find bar, `widgets::fact_row`'s fixed label column — and each either measures its
   text or ellipsises. The pseudo-locale from phase 1 finds them.
7. **Locale formatting** — thousands separators, decimals, `history::relative_time`'s "… ago".
   Nothing is locale-aware today. Decide whether it is worth `icu4x` before starting.

### Data in any script — in scope regardless of locale

These affect users today, with the English UI, whenever a table holds non-Latin text:

- **Grid column auto-fit counts characters** (`grid_char_w`, a fixed 7 px) instead of measuring
  them, so CJK columns come out about half as wide as they should and other scripts mis-size too.
  Measure the sampled strings with `measure_text_px_at`, as tabs and the ERD already do.
- **Fonts.** Only IBM Plex is bundled (Latin, Cyrillic, Greek). CJK, Arabic and Hebrew rely on
  cosmic-text falling back to system fonts, which is unverified per OS; `erd_raster` has its own
  font database holding only Plex Sans, so a diagram PNG probably drops those glyphs.
- **IME in the grid's inline cell editor.** Every other field is `edit_field`, on Floem's editor
  engine, which enables IME and handles preedit and commit (`vendor/floem/src/views/editor/view.rs`).
  The grid's inline editor is the one `floem::views::text_input`, which never calls
  `set_ime_allowed` and handles neither event, so a cell cannot take CJK composition. Either move
  it onto `edit_field` or teach the vendored `text_input` IME (a `PATCHES.md` entry); either way
  the check is by hand, with a real input method.
- **Right-to-left cell data** shapes and reorders through cosmic-text, but always sits
  left-aligned. Aligning a cell by its first strong character is cheap and optional.
