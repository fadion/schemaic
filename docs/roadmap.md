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
- **Settings offers a language only once its catalogue is complete** (`locale::SHIPPED`), and
  so does the OS: a German system gets English until German ships, never a half-German
  interface. While a catalogue is being written it is reached with `SCHEMAIC_LOCALE=de`, as the
  pseudo-locale is with `SCHEMAIC_LOCALE=pseudo`.

### Never translated

SQL of any kind (generated DDL, dumps, exports, formatted queries); SQL keywords and the plan
modal's *Analyze*; persisted names (serde keys, `favorites.json` and the other file names);
`pg_builtins.rs` and `mssql_builtins.rs`, which quote the servers' own text; CLI, MCP and AI text
as above. Key names in shortcut labels follow the language's keyboard legends rather than staying
English — German reads `Strg`, `Umschalt`, `Entf`, `Eingabe`, `Pos1` — through the catalogue's
`key-*` messages, which `shortcuts::keys_label` reads key by key; a key printed the same
everywhere (letters, arrows, `Tab`, `Esc`, `F6`) and macOS's `Cmd` have no message.

### Phases

Each phase leaves English output byte-identical, so the existing suite — several hundred
assertions pin English wording — stays the regression net throughout.

3. **UI literals** in `schemaic-ui` and `schemaic-app` — on the order of two to three thousand,
   heaviest in `widgets.rs`, `overlays.rs`, `lib.rs`, `grid.rs` and `shortcuts.rs`. Mechanical,
   one file per session, by hand: the rule against scripted edits applies in full.
   `source_gate::no_menu_label_ends_in_an_ellipsis` already reads a `tr!` label's text in every
   catalogue, and `overlays.rs`'s `menu_order_gate` keys its skeleton on message ids — the one
   table `schema_tree.rs`'s blank-space menu is placed by too.
   `schemaic-app` has no `i18n.toml` yet and `tr!` is
   `pub(crate)` to `schemaic-ui`; the app's first converted file decides how it reaches the
   loader.

   **Converted:** `settings.rs`, `shortcuts.rs`, `overlays.rs`, `schema_tree.rs`,
   `history_panel.rs`, `snippet_panel.rs`, `tabs.rs`, `activity_panel.rs`, `ai_panel.rs`,
   `chart_view.rs`, `view_editor.rs`, `properties.rs`, `copy_table_view.rs`,
   `database_editor.rs`, `script_view.rs`, `compare_view.rs`, `event_editor.rs`,
   `routine_editor.rs`, `users_view.rs`, `trigger_editor.rs`, `object_editor.rs`,
   `account_editor.rs`, `table_designer.rs`, `connection_form.rs`, `import_view.rs`,
   `dump_view.rs`, `monitor_view.rs`, `result_diff_view.rs`, `ddl_preview.rs`,
   `connection_import.rs`; and, ahead
   of their files, the three
   workspace panel headers — `lib.rs`'s terminal and results headers (`term-*`, `results-*`) and
   `editor_pane.rs`'s query header (`query-*`). The schema editors share
   `editor-preview-sql` and `editor-change-count`, and `dialog-close` joins the dialog words. The
   activity panel's counts line ("18 sessions · 4 running") and its session-state words stay
   core's English with phase 4: the total is core's `ActivitySummary::total_label`, and the
   state word is also what the search box matches. A palette
   command's typed name is its translated label lowercased, and its English name
   (`Command::id`) matches in every language too, as VS Code's translated palette does. Shared
   words live under `dialog-*` (Cancel, Yes, No, and `dialog-read-only` — the sentence beside a
   disabled write button, which the import modal and the DDL preview share), and the schema
   tree's menus and panel under
   `tree-*`. `section_title` and `loading_dots` take owned text now, so the other panels'
   headings can follow.
   **Left, in order** — delete a name as its file lands: the remaining modals by size,
   `blob_view.rs`,
   `erd_view.rs`, `server_vars_view.rs`, `snippet_edit.rs`, `plan_view.rs`, `used_by_view.rs`,
   `themes.rs`, `markdown.rs`, `cell_editors.rs`, `alias_rename.rs`, `usages_view.rs`,
   `tooltip.rs`, `erd_raster.rs`; then the four large files' remainders, `grid.rs`, `lib.rs`,
   `widgets.rs` and `editor_pane.rs`, each more than one session; then `schemaic-app`
   (`main.rs`, `install_cli.rs`, `ai.rs`, `conn_sources.rs`, `notify.rs`, `agent_cli.rs`,
   `antigravity.rs`), whose first file settles how it reaches the loader (above). A file with
   no literal a user reads drops off without a commit. The list came from a sweep for
   capitalised string literals, so sweep a file whole before calling it done; a sentence that
   starts lower-case, or spans a `\` continuation, slips past a quick one.
   **How a file goes**, as those settled it: a helper taking
   `&'static str` widens to `impl Into<String>` (every literal caller still compiles); a
   placeholder goes through `FieldCfg::placeholder_of(|| tr!(…))`, whose `placeholder` is still
   `&'static str`; a sentence assembled from parts becomes whole messages, one per variant; a
   count becomes a Fluent plural. Every message is one line in the `.ftl` — a continued Fluent
   value keeps its line break, where the Rust `\` it replaces kept none. German is written in the
   same change. A `const` table of labels keeps its identity and loses its text: a string the
   code or a test matches on becomes an enum with a `label()` (`shortcuts::Group`), and the text
   becomes a `fn() -> String` closure calling `tr!` (`shortcuts::Describe`), so the table stays
   `const` and the id is still checked at compile time. Text that is not the file's own stays: labels a dropdown takes from another
   module or crate (`TermCursor::label`, `Agent::label`, `KeywordCase::label`), and core's
   prose (`statement_timeout_label`, the install status), which is phase 4. Afterwards, compare
   each placeable-free message against the file's literals at `HEAD` with `\` continuations
   joined — the suite pins too little of a page's wording to show a changed word.
4. **Messages built in `schemaic-core`.** The hard part. Around 130 functions return finished
   English prose (`run_verdict`, the DDL preview notes, `Rollback::note`, `tx`, `dump`, `compare`,
   `stats`, `import`, `conn_import`, `users`), and core cannot — and should not — reach a UI
   catalogue. `run_verdict`'s `Block(NO_DATABASE_SELECTED)` is the one the CLI already
   recognises by its text (`cli::run::refused_for_no_database`) — harmless while the CLI is
   English, and the first thing a typed refusal replaces. Each becomes a typed message (an id plus arguments, or an enum per area) the UI
   renders; core keeps an English rendering for the CLI and MCP. Settle the message type on one
   area first and review it before the rest follow. The existing enums are the nearest starting
   points, though they render in three different ways: `AliasRenameRefusal` through `message()`,
   `ImportError` and `ParamError` through `Display`, and `FilterError::BadCondition` not at all
   — it carries a finished `String` (the parser's text, or a sentence built where it is raised),
   so its prose lives at its construction sites and it is the weakest of the four to start from.
   **A word that is a property of a typed value needs none of that and goes early**: the
   interface draws its own (`i18n::object_kind`, `object_kind_group` for `ObjectKind`), core
   keeps its English for the CLI, MCP and AI, and a test holds the two equal in English. Done for
   `ObjectKind`, `history::Bucket` (`history_panel::bucket_title`), `activity::KillKind`
   (`activity_panel::kill_label`), `chart::ChartKind` (`chart_view::chart_kind_label`) and
   `compare::CompareKind` and `EmptyRows` (`compare_view::compare_kind_group`,
   `compare_kind_word`, `empty_rows_message`), `users::PrincipalKind`
   (`users_view::principal_kind_label`), and `GrantSubject` and `GrantLevelKind`
   (`account_editor::grant_subject_label`, `grant_level_label`), `ddl::IndexStorage` and
   `ddl::identity_wording` (`table_designer::storage_label`, `identity_wording`). Where a
   core function composes a line around one counted word, it takes the word from the caller
   instead (`history::outcome_line_with`, which `i18n::outcome_line` calls);
   `ExportFormat::label`, `activity::interval_label`, `KeywordCase::label` and the
   like are the same case. Keys built from core's word (the tree's `objgrp:`/`obj:` expansion
   keys) and AI prompts keep core's English.
5. **Plurals and assembled sentences** — a few hundred sites using `plural(` or `== 1 { "" }`,
   folded into whole Fluent messages as their phase-3 or phase-4 file is converted rather than as
   a pass of their own.
6. **Layout for longer text.** German, Finnish and Russian run 30–100% longer than English.
   Roughly sixty fixed-width boxes sit beside text — the connection form, the import and compare
   panes, the find bar, `widgets::fact_row`'s fixed label column — and each either measures its
   text or ellipsises. The pseudo-locale (`SCHEMAIC_LOCALE=pseudo`) finds them.
7. **Locale formatting** — thousands separators and decimals. (The "… ago" is done: core's
   `history::ago` is the interval as a value, `i18n::relative_time` says it in the interface's
   language, and `relative_time` stays core's English.)
   Nothing is locale-aware today. Decide whether it is worth `icu4x` before starting.

### Data in any script — in scope regardless of locale

These affect users today, with the English UI, whenever a table holds non-Latin text:

- **Fonts on screen.** Only IBM Plex is bundled (Latin, Cyrillic, Greek). CJK, Arabic and Hebrew
  rely on cosmic-text falling back to system fonts, which is unverified on macOS and Linux. (The
  diagram PNG has its own fallback now, `erd_raster::export_options`.)
- **Right-to-left cell data** shapes and reorders through cosmic-text, but always sits
  left-aligned. Aligning a cell by its first strong character is cheap and optional.
