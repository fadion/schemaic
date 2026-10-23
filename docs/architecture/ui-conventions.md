# UI conventions

- **No pointer cursor on buttons/icons** — native apps keep the arrow cursor; a pointer feels
  web-like. Use the default; reserve `CursorStyle::Text` for text inputs (a genuine hyperlink may
  keep `Pointer`). The account browser's rows shipped with `CursorStyle::Pointer` and are named here
  because a rule with no instance reads as a preference. **It is a source gate**, because a `grep`
  nobody ran was not a check: this bullet called one sufficient while six sites had slipped back in
  — the Server Activity and Server Variables tabs, the reload-from-disk offer's buttons, the
  settings model chips, and the grid's figure copy and hidden-columns corner.
  `source_gate::tests::no_pointer_cursor_outside_a_hyperlink` scans `crate_sources`' production code
  and fails on any `CursorStyle::Pointer` without an `if is_link` in the 120 bytes before it. That
  one exception is the terminal's hyperlink run in `lib.rs`, and the gate asserts exactly one such
  site, so a rewording that stops it matching fails rather than reporting success.
- **A selection outline is painted in both states; only its colour changes.** Taffy sizes the
  **border box**, so a 1px rule added only while a control is picked grows it 2px: a button sized by
  its own padding jumped a pixel each way the moment it was chosen and the row re-flowed around it,
  which is how every toggle in the two account forms shipped.
  `account_editor::picked_outline(style, picked)` is the one helper, and it paints the border
  always, `theme::accent()` when picked and `Color::TRANSPARENT` when not, which leaves
  the colour as the only thing that moves. Its remaining caller is the grant form's `privilege_tag`
  — Kind, Level and the two option rows are now the app's `<select>` and its switch — and that
  wrapping tag cloud is the reason the helper is still here: the tags **wrap**, so a tag that grew
  as it was picked would re-flow every tag after it on the line, not just itself.
  This is the same accounting `widgets::row_menu_mark_pad`
  does for a tree row (*Popup menus*), taken from the other end — that row's height is fixed by what
  surrounds it, so the 2px is given back out of its padding, while a button sized by its own padding
  has nothing to give back and needs the border there all along.
- **`btn_primary_text` is not "white on the accent".** It is the *label* colour of the Primary
  button, chosen against `btn_primary`'s own dark navy fill — `#8EA7EA`, a light blue. Painting it
  on a saturated fill leaves a glyph nobody can see, which is exactly how the import list's
  checkbox shipped its first draft: a filled blue square with an invisible tick in it. The pair
  that *does* mean "on, and read me against it" is `toggle_on` + `toggle_handle_on`, the second of
  which is `#FFFFFF` in both themes precisely because it is drawn on the first — so a new filled
  control that has to show something inside itself takes the toggle's colours, not the accent's.
  Small glyphs drawn inside one also want `icons::CHECK_BOLD` over `icons::CHECK`: Lucide's
  stroke-width 2 is tuned for ~16px and falls under a device pixel at 11. The result is
  `widgets::check_box`, and it is **the** checkbox: a multi-select list draws that, never a box of
  its own, which is what the dump modal's table picker was until it adopted it.
- **Menu labels carry no trailing ellipsis**, even when the entry opens a dialog. The platform
  convention says a `…` means "this will ask you something first", but this app doesn't keep it:
  of the menu labels in the two view crates, the handful that ever had one were each the odd one
  out, added and later removed. Follow the count, not the platform guideline — a menu where a few
  entries out of a hundred trail dots reads as an inconsistency, which is what it is. **It is a
  source gate**, because written down was not enough: the grid's AI sparkle menu carried
  `AI seed table…` past this bullet, and `source_gate::tests::no_menu_label_ends_in_an_ellipsis`
  was seen red on exactly that label. It scans both view crates' production source (`crate_sources`) for every
  `MenuEntry::<constructor>(` whose first argument is a string literal and fails on one ending in
  `…` or `...`, with a floor of 50 labels scanned so a scan that stopped matching cannot report
  success. **The rule follows a label into the catalogue**: a first argument of `tr!("id"…)` has
  that id's text checked in *every* catalogue (`catalogue_messages`, the single-line messages of
  each `i18n/*/schemaic.ftl` — the repository-source exemption), since a translation that adds the
  dots is the same habit in another language; given an ellipsis, the German `tree-hide` failed it
  as `tree-hide (de)`. It reads source, so a label built at run time is not seen.
- **The footer's panel toggles: a control that can't act must not look like one, and when the user
  can fix that, say so.** Below their breakpoints the side panels are *force-hidden* — the schema
  tree at `panels_min_schema_w`, the right column at the wider `panels_min_full_w` — and the window
  narrowing past one is something the user does by dragging an edge, not a rare state. Every guard
  for it was already in place (`schema_panel_allowed`/`right_panel_allowed`, read by `body`, the
  dividers and each toggle's `active`), so the toggles were *correct* and silently did nothing: five
  status-bar icons at full brightness whose clicks changed a signal nothing renders, which reads as
  a broken button rather than as a consequence of the window size. They now wear
  `toggle_icon`'s disabled face and carry a tooltip saying to widen the window.
  **Every toggle names its panel otherwise** — "Schema", "AI assistant", "Query history", "Snippet
  library", "Server activity", "Terminal", each panel's own title in sentence case — because an
  icon alone did not say what it opened until it was clicked (GitHub #11). So the tooltip is always
  there, `PanelToggle::tip` is a plain `&'static str` rather than the `Option` it was, and the
  footer's call sites are a bare `.tooltip(move || text(…tip))`, not `widgets::tip_when`. The tips
  open *above* the icons — there is no room below the footer, and floem's overlay pulled them back
  up over the icon itself (`tooltip.rs`, `tip_top`).
  **`panel_toggle(panel, fits, offered)` answers the face and the tooltip in one call**, and that
  is the point of it rather than a convenience: they are the same fact, so two predicates could
  disagree in two bad ways — a dimmed toggle explaining nothing, and a live toggle insisting the
  window is too narrow for it. Unit-tested over every input in `panel_toggle_tests`, including the
  invariant that `PANEL_NARROW_TIP` never accompanies an enabled toggle
  (`the_narrow_tip_never_appears_on_an_enabled_toggle`). **Only the narrow case explains itself**,
  its sentence standing in place of the name, deliberately: it is transient and actionable, while
  Server Activity on an engine with no sessions is permanent and keeps `toggle_icon`'s original
  ruling that *a toggle which opens an explanation is a worse answer than one that visibly isn't
  offered* — that toggle is dim and only *named*, and naming explains nothing, so the ruling stands
  (`a_panel_the_engine_does_not_offer_is_disabled_and_only_named`). When both apply the narrow one
  speaks, being the half the user can act on.
  **Find-Anywhere's Toggle Panel rows answer the same state, in the palette's own idiom:** dimmed
  and skipped, with no tooltip — a list row is not a control with a hover affordance, and the rows
  beside it are the explanation. They read the same two predicates, so the palette and the status
  bar can never offer different answers. Note the palette now has **two** answers to "unavailable"
  and they are not interchangeable: *omission* for Server Activity on an engine with no sessions,
  because that panel does not exist for the connection and a row would be a route the mouse
  refuses; *dimming* for a window that is too narrow, because the command is real and will work
  again in a moment — and dropping those rows would make the list's length jitter while the user
  drags a window edge. `PaletteItem::enabled` carries it, `widgets::list_step_enabled` and
  `first_enabled` keep the arrow keys off dead rows (unit-tested, including a cursor left on a row
  that has just gone dead), and `open_sel` refuses one that Enter still reaches.
- **A tooltip that appears only sometimes needs `widgets::tip_when`, not a branch.** The app's
  `.tooltip()`, like the floem one it was copied from, has no "not now": once the hover delay fires
  it always adds the overlay, and an empty tip is a small empty box because `AnchoredTooltip`
  applies `tooltip_style`'s chrome to whatever root it is handed.
  The two older conditional tips (a truncated ERD header, a tab's path) decide **once, at build**,
  and an `AnyView` branch is right for them. `widgets::ellipsized_text` cannot: whether its line is
  cut depends on the width it was laid out at, so it decides at hover through `tip_when` (under
  `widgets.rs`). A condition that changes while the app runs — a window
  width — cannot use that shape, and does not need to: `AnchoredTooltip::update` calls the tip
  closure at the moment the delay fires, so a signal read there is read fresh on every hover with no rebuild underneath. What
  it returns when there is nothing to say is a `display: none` root, which the chrome cannot
  override — floem hands each `.style()` closure a fresh `Style` and merges results per property by
  push order, and `tooltip_style` sets no `display`.
  **It is generic over the tip's text (`S: Into<String>`), and it was not, which is how the rule got
  restated wrongly at the two sites that could not use it.** The header's connection switcher shows
  the connection's *name* — an owned `String`, filtered to `None` when nothing was elided — so it
  went round this helper with a bare `.tooltip(|| text(conn_tip()))` returning `""`, under a comment
  claiming that an ordinary name raised no tooltip at all. Every name of fifteen characters or fewer,
  which is nearly all of them, therefore drew a small empty bordered chip on the app's most-hovered
  control; the results strip's chip had the same shape, falling back to `String::new()` for a panel
  already removed. Both go through `tip_when` now, and so does a **third** that the fix missed:
  each row of the connection switcher menu in `overlays.rs`, whose tip — the full name and
  endpoint, when either was elided — was a `String` left empty otherwise and handed to a bare
  `.tooltip(…)`, so every ordinary row drew the same empty box on hover. It is an `Option<String>`
  now, `None` when nothing was cut. **This class has no gate, and the gate that was
  written for it is why**: it scanned each `.tooltip(` closure for an emptiable expression and
  *passed against both of the bugs it was written for*, because both spelled the emptiness in the tip
  binding above rather than inside the closure — and nothing mechanical distinguishes "a tip that can
  be absent" from "a tip that is a non-empty `&'static str`". It was deleted rather than shipped, and
  that is said here rather than implied by a green test.
- **Labels aren't selectable** — Floem's `Selectable` defaults to *true*, so every caption/header/tree
  row would drag-highlight like a web page. The workspace root sets `.class(LabelClass, |s|
  s.selectable(false))`, which cascades to the whole tree (and, via the captured context style, into
  dropdown popups); `tooltip_style` repeats it because a tip is an overlay outside that tree, styled
  by `tooltip_style` alone.
  Text selection belongs to real text surfaces — `edit_field`, the SQL editor, and the
  terminal (which paints its own selection). Don't re-enable it on a label.
- **Colors live in `theme.rs`** as named fns — add one rather than inlining a hex literal. They read
  the *active* theme from `themes.rs` (reactive), so calling one inside a `.style(…)` closure follows
  a live theme switch for free.
  - **Reuse a theme color only when the shared scope makes sense** (the two spots are the same role and
    should always change together). Otherwise extract a separate named fn — even if it starts at the
    same hex — so each can be retuned independently (e.g. `seed_button()` alongside the identical
    `status_ok()`).
- **Theming (`themes.rs`)**: three independent axes — `UiTheme` (chrome: dark/light, including the
  short `sql_*` palette for SQL shown outside the editor), `EditorTheme` (the editor's surface and
  syntax tokens, and nothing outside the editor: One Dark Pro / Tokyo Night / Catppuccin Latte) and
  `UiScale`
  (how large the chrome is drawn). A theme is a flat struct of named colour roles (hex). All three
  live in `Scope`-owned global `RwSignal`s; `theme::set_ui`/`set_editor`/`set_ui_scale` swap them,
  and all three are **idempotent** — a re-pick of the value already in effect does nothing, which is
  the guard's own entry under *Floem 0.2 gotchas* (`RwSignal::set` never dedups) and the reason
  `ThemeState` stores `ui_kind`/`editor_kind` beside the built themes at all: a built `UiTheme` has
  no identity to compare against. The choices are persisted (`ui_theme`/`editor_theme`/`ui_scale` in
  `UiState`) and seeded via `theme::init` before the view builds. Editor tokens re-highlight on switch because
  `SqlStyling::id()` folds in `theme::editor_generation()` (`layout_cache_key`, with the UI
  generation, the font size and — read tracked off the tab's dialect memo — the engine, so a tab
  moved to another kind of engine re-lexes its lines; the styling used to hold the dialect it was
  built with).
  - **Live-switch caveat**: a colour read *inside* a reactive `.style` closure updates instantly; one
    captured *by value* freezes at build time. Prefer `fn() -> Color` for anything themable (see
    `FieldCfg::background`).
- **Interface scale (`UiScale`: 80% / 100% / 130% / 160%)** multiplies the
  *design tokens*, not the window. Floem 0.2 has no user-settable render scale, and a paint
  transform over the whole tree would take hit-testing and the editor's own coordinate arithmetic
  with it — so instead the type scale (`theme::font_*()`), the layout metrics (`consts::*`), the
  per-module modal metrics, `icons::icon` and every `padding`/`gap`/`margin` in the views (~600 call
  sites, each wrapped as `theme::scaled(…)` *inside* its style closure) all read the signal and round
  to whole pixels (`themes::scale_at`, unit-tested: **`Normal` is the exact identity**, results are
  integral, and a positive size never rounds to nothing). The air between things grows with the
  things, which is what stops a 160% window reading as big type crammed into 100% boxes.
  - **That rounding is not size-preserving below 1.0, and the type scale collapsed at 80%.** Four
    base values a single pixel apart (14/13/13/12/12) all came back **10.0** at `UiScale::Small`, so
    `font_body`, `font_label`, `font_hint` and `font_status` were one size. Two statements in
    `theme.rs` were then false: `font_hint`'s own doc, "one step under the label it explains", and
    the section header's "nothing smaller than `font_body()` anywhere except the status-bar footer",
    whose carve-out was empty because the footer was no longer smaller either. Every form hint and
    every footer segment rendered at label size, at the one setting chosen by someone who wants more
    on screen and can least afford four type sizes reading as one. The fix is
    `min(font_body() - 1.0)` on the two accessors rather than a wider base, and **only the scale
    that collapsed moves**: 80% goes 10 → 9, while 100% (12), 130% (16) and 160% (19) are the
    numbers the app already shipped and a base change would have moved everywhere.
    `themes.rs` already pinned each token's rounding individually (13 → 10 at Small is asserted
    there) and nothing compared two, which is why the collapse was pinned and unexamined;
    `the_type_scale_keeps_its_steps_at_every_scale` asks the property through the accessors at every
    `UiScale` and reports *"80%: a hint 10 is not one step under the label 10 it explains"* without
    the `min`. Whether 9px reads acceptably is a judgement, and is a hand check in
    `review/user-verify-fix.md` rather than something a test can hold.
  - **What it deliberately doesn't touch**: the SQL editor's code font and the terminal font, both of
    which already have their own size setting (and the editor a per-tab Ctrl+scroll zoom whose
    override is an absolute px that a second multiplication would double-apply); persisted panel
    widths, which are px the user dragged — the *minimums* they clamp against scale, which is what
    keeps a panel from being narrower than its own text; and the ER diagram's canvas, which has its
    own zoom. The terminal's **cell metrics** ride on that first exemption: `term_cell_wh` derives
    the cell from a font size that never went through `scaled_font`, so it is already in the right
    units and must stay unwrapped.
  - **The other half of a mixed surface is the trap: a *scaled* length beside an unscaled one must be
    read, never restated.** The terminal is exactly that surface — unscaled cells inside a scaled
    `term_pad()` — and the two functions that wrote `6.0`/`12.0` instead of reading the metric were
    right at 100% and wrong everywhere else, handing the PTY columns the panel could not draw and
    mapping clicks a whole cell off across the right-hand part of every row. A literal that equals a
    scaled token at `Normal` is the one kind of wrong number that survives review, because the only
    scale most reading happens at is the one where it is correct. `term_fit`/`term_cell_at` take the
    padding as a parameter for that reason; the whole of it is under `ui/lib.rs`.
  - **Shapes and hairlines stay literal**, which is the line the padding sweep stopped at. A
    `border_radius` is a *shape*, not air — scaling a 5px corner to 8px reads as a different design
    rather than a larger one — and a 1px rule or border is a hairline at every scale, so `.height(1)`,
    the menu separator's rule, the menu panel's border and `edit_field`'s `+ 3.0` (two borders plus a
    pixel of rounding slack) are all deliberately unwrapped. `menu_panel_height` is where the
    distinction is visible in arithmetic: its boxes scale, its two borders don't. **The exception is
    a radius that is half a scaled box** — a circle or a pill, which stops being one the moment the
    box grows and the radius doesn't; see the interface-scale invariant for the five sites and for
    why no grep finds them.
  - **A modal's size scales, then caps against the window** — `widgets::modal_w` for the width,
    `modal_h` for a fixed-height panel, `modal_body_h` for a scrolling body inside one. All three
    read `window_size()` inside the caller's style closure, so a resize re-runs them, and all three
    clamp their own floor to the window (a *scaled* floor passes 500px, and one wider than the
    screen would clip through the guard meant to keep the panel usable). **On the vertical axis the
    extent is the modal *layer*, not the window** (`cap_to_window` measures `modal_layer_h()` — the
    window less `header_h()`): since `07bda98` every modal hangs in a layer inset by exactly the
    `scaled(40.0)` that is also `modal_h`'s reserve, so measured against the window the layer spent
    the guard's whole budget before the panel was sized. On a roomy window the panel came out
    *exactly* as tall as its container, flush under the title bar and flush against the window's
    bottom; on a short one the floor — clamped to the window — made it taller than the container with
    nothing to clip it back, putting the footer where Apply lives off the bottom edge. Only the
    height needs the inset, the layer being full width
    (`a_modal_fits_the_layer_it_is_centred_in_not_the_window`; three existing tests had asserted the
    pre-`07bda98` geometry and were corrected with it).
    - Heights were left unscaled at first, on the grounds that 620px is already most of a laptop
      screen — but the type inside them grew, so at the 200% then offered an editor was three
      fields and a scrollbar.
    - **Width is the one that must fit first.** A modal is centred in a full-window backdrop: one
      taller than the window loses its footer (where Apply lives) off the bottom, but one *wider*
      than the window loses its left half — the designer's list pane and every field label with it.
      The 900px editors came to 1800 at the 200% then offered, wider than a 1440p screen's window; at
      today's 160% top scale they come to 1440, which a 1366px laptop still cannot hold.
  - **A size the scale has to reach cannot be a plain `f32` parameter.** Three carry a `fn() -> f32`
    for exactly the reason the colours do:
    - `FieldCfg::font_size` — `edit_field` derives its box height, padding, placeholder position
      **and** its editor's `FieldStyling` from it (a hand-written `Styling`, because floem's
      `SimpleStyling` takes the size by value);
    - `widgets::highlight_text` / `highlight_mono` — their `rich_text` closure *is* reactive, but
      only for what it reads inside itself;
    - `widgets::loading_dots` / `verb_spinner` — which also *measure* from it (the reserved width
      that stops the dots reflowing), and which live for as long as the operation they report.

    All three shipped as `f32` and all three froze at the size their view was built at: every text
    field kept 13px type in a box grown for 20px, every highlighted row in the tree / history /
    activity panels kept its old size until a filter change rebuilt it, and the app's one moving
    indicator stayed at whatever scale was active when the query started.
  - **`set_ui_scale` bumps `ui_generation`** for the one place a size still can't be re-read: a text
    `Attrs` list built outside a reactive closure (`markdown.rs`). Views keyed on the generation
    rebuild.
  - **A *structural* decision taken against a scaled measurement has to be in the container's
    rebuild key**, because it is the one kind of answer a `.style` closure cannot give: whether to
    attach a tooltip at all, or how wide a scroll range is, is not a property that re-runs.
    `editor_pane`'s Ctrl+K / inline-AI key carries `theme::ui_generation()` because `diff_view`
    measured `content_w` from the font at build time and baked the diff's syntax colouring into a
    text `Attrs` list — without it a live scale change left the rows rendering at 1.6× inside a
    `min_width` computed at 100%, so the end of a long line could not be scrolled to. **Both of
    those measurements went with `diff_view`** when the suggestion moved into the editor's line flow:
    the rows are laid out by the editor at the *editor's* own font (which the interface scale does
    not touch) and their colours are resolved inside `inline_diff::segments` on every call. What is
    left in that container is a bar and a footer whose every metric is read inside a style closure
    (`theme::scaled(…)`, `icons::icon`'s own closure, `FieldCfg`'s `fn() -> f32` sizes), so **the
    key was dropped with the measurements** — read the Ctrl+K story as the case that *taught* this
    rule rather than as a live instance of it. `tabs`' chip
    key carries `theme::ui_scale()` **and the presence of the DB-identity dot**, for the same reason
    from the other side: `truncated` is computed once at build and decides whether the chip gets a
    tooltip, and all three terms of that comparison move — the measured title, `tab_title_avail()`,
    and the `tab_dot_w()` the dot sheds from it — so assigning a colour to the open tab's database
    narrowed the title enough to ellipsize it while `truncated` still said it fitted, and the one cue
    that says what the clipped title is never arrived. The dot is read there as a *presence*, not a
    colour: the swatch is an ordinary style read and only the width it costs is structural.
  - **A cursor menu's flipped arm pins a *trailing* inset; it never computes a leading one**
    (`widgets::cursor_menu_insets` → `MenuInset::Start`/`End`). Four arms per axis: after the cursor
    if it fits, before it if that fits, flush with the window's far edge when neither does, and the
    window origin for a panel bigger than the window. The panel's size is an **estimate**
    (`menu_panel_height` counts rows), so it is allowed to decide *which* edge and nothing else —
    subtracting it from the cursor put the real edge wherever the estimate was wrong, which showed
    as a gap between a flipped menu's bottom and the pointer that flipped it. `submenu_insets` has
    always worked this way; this is the same trick, and the reason that one never drifted.
    - **A panel dropped from a *box* flips above the box's top, not above the point it drops from**
      (`widgets::box_menu_inset`). A cursor is a point, so `menu_inset`'s single anchor is right for
      it; a control has two edges, and reusing the bottom one for the flipped arm puts the panel
      across the control — which, when the control is the button that opened *and* closes the panel,
      means the panel covers its own dismissal. Two callers: the date field's calendar
      (`calendar_insets`) and every menu anchored `PopupAnchor::BelowBox` — the row panel's enum,
      boolean and set pickers, and an open grid cell's. Both anchors therefore carry the control's
      **top** edge as well as its bottom. `BelowIcon` does not and stays on `menu_inset`: its panel
      is meant to overlap its 28px glyph, which is the whole difference between the two variants.
    - Two earlier spellings both failed at scale, and both are worth not re-deriving: clamping the
      flipped arm at zero threw every context menu to the window's top-left corner once a menu was
      600–750px tall, and scaling the whole 30.5px row estimate (padding included, back when the
      paddings were still literal) over-predicted a long menu by ~190px and flipped menus that had
      room below them. The estimate is still summed from its parts now that the paddings do scale,
      for a smaller but live reason: each part rounds to its own whole pixel the way the style that
      draws it does, and the panel's two 1px borders must *not* grow with the boxes.
  - **The interface scale is a segmented control, not a dropdown** (`settings::scale_picker`),
    because floem nudges an overflowing overlay in paint only — see *Floem 0.2 gotchas*, where the
    finding is written up in full. Four short segments, one Tab stop with Left/Right inside it
    (`nav_group`), arrows applying as they move. The segments are labelled with the **percentage
    itself** (`UiScale::label`) rather than Small/Normal/Large/Huge: the number is what the control
    is asked about, four of them fit the row at every scale where four names didn't, and a label
    computed against `factor()` can't go stale when a factor is retuned — which is why the variant
    names (internal only) and the persisted `key()`s (semantic, so a retune carries a stored choice
    rather than resetting it) are deliberately *not* the same vocabulary.
  - **The AI panel's height floor is held only while a turn streams** (`next_floor(…, !busy)`).
    `ai_panel`'s `floor` exists for the measurement dip a rebuilt `RichText` reports mid-stream; an
    idle panel has no dip to hide, so it takes what it just measured. It *also* releases early,
    inside a stream, on the things known to change the true height (message count, wrap width,
    connection, interface scale) — but that list is an optimisation now, not the guarantee, because
    it was incomplete twice: first the wrap width, then the scale, each leaving a **permanent** band
    of blank under the last message rather than one frame of it. Written as the premise, a missed
    invalidator closes itself on the next layout.
  - Derived metrics sum the **scaled parts** rather than scaling the sum (`consts::leaf_pad`), so an
    indent lands on the pixel the glyphs it aligns under actually occupy — at 80% those differ by one.
- **The string catalogue: a translated label is spelled `tr!("id")`**, or `tr!("id", name =
  value)`, never a literal (`ui/i18n.rs`; which language is `core/locale.rs`). **Every file of
  `schemaic-ui` and `schemaic-app` now draws what a user reads from the catalogue** — but for
  `ExportFormat::label`, whose names are English on purpose (under `overlays.rs`) — so a new word
  a user reads goes in through `tr!` as well, with every translation's words in the same change —
  eight catalogues beside `en-GB` now, not one; a message a translation lacks falls back to
  English, so this is a convention no test enforces — a literal is
  the odd one out now, and under a translated catalogue (`SCHEMAIC_LOCALE=de`, below) it is the
  one that still reads English. The app reaches
  the one loader through its own `tr!` (the `schemaic-app` entry says why it cannot borrow the UI
  crate's), and the app's messages live in the same two `.ftl` files. Only the GUI is translated:
  CLI, MCP and AI text stays English, as do SQL and persisted names; key names follow the
  language's keyboard legends through the `key-*` messages. **Text the GUI shares with one of
  those surfaces — the export and compare notes — is split at that seam, never translated
  through it**: a typed value each side words, so a `schemaic exec` report a script parses stays
  English. **A server's or driver's error stays in the server's language**, so a translated
  sentence wrapped round an English (or German, or whatever the server is set to) error is
  expected, not a defect.
  - **How a file draws from it** — the shape the whole conversion settled on, file by file, and
    the one a new view follows. A helper taking `&'static str` widens to `impl Into<String>`, so
    every literal caller still compiles and a `tr!` result passes too (`section_title`,
    `loading_dots`, the settings rows). A placeholder goes through `FieldCfg::placeholder_of(||
    tr!(…))`, `placeholder` itself staying `&'static str`. A sentence assembled from parts becomes
    whole messages, one per variant, since a translator can reorder a whole sentence and not a
    splice; a count becomes a Fluent plural rather than `text::plural`. **A message's text is one
    line in the `.ftl`** — a plural's selector arms aside, which are syntax and draw no break: a
    continued Fluent value keeps its line break, where the Rust `\` it replaced kept none, so
    wrapping a long message in the catalogue changes what is drawn. Where a real line break *is*
    wanted, it is two messages joined in code, as `main.rs`'s keyring notice on deleting a
    connection joins `conn-delete-keyring-kept` and `conn-delete-keyring-later`. **A `const` table
    of labels keeps its identity and loses its text**: a string the code or a test matches on
    becomes an enum with a `label()` (`shortcuts::Group`), and the text a `fn() -> String`
    non-capturing closure over `tr!` (`shortcuts::Describe`, `CONN_COLOR_PRESETS`' names), so the
    table stays `const` and the id is still checked at compile time. **An API that takes only
    `&'static str`** — floem's `FileSpec::name`, a file dialog's type filter — gets `tr_static!`:
    a `OnceLock` per use site, filled once, which is sound only because the language is fixed at
    launch, and leaks nothing per dialog opened. There are two, the same macro each over its own
    crate's `tr!` — the app's at the top of `main.rs`, and `schemaic-ui`'s in `ui/i18n.rs`, which
    `script_view.rs`'s *Run a SQL script* dialog and `dump_view.rs`'s save dialog use (both for
    `file-sql-filter`) and `tr_static_reads_the_message_once_per_site`
    holds to the message. A word core gives
    as a property of a typed value is drawn UI-side (below), and so is a message core gives as a
    typed *why*; prose core hands over as a finished `String` is not the view's to translate and
    stays as it comes.
  - **Shared words have shared ids**, so one action cannot be worded two ways: `dialog-*` for
    the words no one dialog owns (`dialog-cancel`, `dialog-yes`, `dialog-no`, `dialog-close`,
    `dialog-stop`, and `dialog-read-only`, the sentence beside a disabled write button that the
    import modal and the DDL preview share), `tree-*` for the schema tree's menus and panel, which
    the context menu and the SCHEMA gear both draw, and `editor-preview-sql` and
    `editor-change-count` for the schema editors' footers. A palette command's typed name is its
    translated label lowercased, and its English `Command::id` matches in every language too
    (under `overlays.rs`).
  - **What stays English on purpose**, so a sweep for capitalised literals does not "fix" it: SQL
    and its sentinels (`WHERE`, `<null>`, `<default>`, `SQL VALUES`/`UPDATE`/`WHERE IN`, the
    parameter chip's `abc`/`123`/`NULL`/`SQL`); the export formats' names; persisted keys and file
    names; font names; `pg_builtins.rs` and `mssql_builtins.rs`, which quote the servers' own
    text; the terminal's escape sequences; the find options' glyphs `Aa`, `W` and
    `.*`; product names (the agents', *Microsoft Entra*, *AWS IAM*, *Google Cloud IAM* and the CLIs
    the sign-in picker names); **everything sent to an AI model** — the
    Explain prompt, the Optimise intent (whose box label `cmdk-optimise-label` *is* translated and
    compared against the box, so the English intent still goes out), the AI fill and seed system
    prompts, every prompt `ai.rs` and `antigravity.rs` send — and the converse holds: Schemaic's
    own words in a turn *are* worded, so they are `transcript::TurnNotes`, which the replay into
    a later prompt never reads, rather than prose that it does; `--mcp-serve`'s errors to an agent's
    stderr; what the app writes to the write log, a persisted record; log and `tracing` lines;
    the prose core and `schemaic-cli` return wherever the CLI, the MCP server, a log or a model
    reads it — where the window shows it, it is a typed value the window words (*A typed
    message*, below); and four `Query`
    sentences `schemaic-db` deliberately left plain, three of them internal invariants failing
    (`COUNT(*) returned no row`, *the count came back as … not a number*, *A copy needs the
    multi-threaded runtime*) and MySQL's `Unknown database '{db}'`, which mimics the server's own
    text — only their *query failed:* is worded. A figure is not
    among it: core prints it in English and the view writes it in the language's marks (*Figures
    in the interface's marks*, below). The notes under
    the connection form's fields are no longer among it (`connection_form.rs`), nor are
    `launch::*`'s refusals and `external_client_refusal` (`launch::Refusal`, below), nor the
    startup and keyring notices (`persist::Notice`, below), nor the file import's refusals, memory
    warnings and problem-list tail (`import::ReadRefusal`, below), nor the plan modal's warnings
    (`plan::PlanNote`, below) and SQL Server's plan table (`plan::PlanWording`, below), nor the
    run guard's reasons (`sql::GuardReason`, below), the
    read-only gate's `ReadOnlyReason` inside one included, nor the filter field's two refusals
    (`filter::FilterError`, below), though `sqlparser`'s text in a `BadCondition` still is, nor the
    account browser's notes (`users::PrincipalsNote`/`GrantsNote`, below) and attributes
    (`users::AccountAttribute`, below), nor any object editor's
    refusals (`ddl::DatabaseProblem`/`ViewProblem`/`EventProblem`/`ObjectProblem`/
    `RoutineProblem`/`TriggerProblem`/`TableProblem`, below), though every draft's `validate`
    still answers core's English, nor the editor's diagnostics (`intel::DiagMessage`, below),
    though the parser's detail in a syntax error and the server's words inside its error still are, nor a
    write path's failure (`model::WriteFailure`, below), though the server's words inside it
    still are, nor `DbError`'s own *query failed:*, *connection failed:* and *query cancelled*
    wherever the app shows an error through `db_error_shown` (below) — the sites still on
    `to_string()` are listed there — nor `schemaic-db`'s own refusals (`model::DbRefusal`, below),
    `AuthMode::transport_refusal`'s sentence among them, nor the sentences it composes around a
    driver's, the SSH library's or the system's text at connect (`model::ConnectFailure`, below),
    its cloud IAM and Entra sign-in sentences among them, nor the `Query` sentences it writes
    itself (`model::QueryFailure`, below) but the four above,
    nor the DDL preview's change lines (`ddl::ChangeLine`,
    below), risk lines (`ddl::Risk`, below), risk heading, *Used by* lines (`used_by::Listing`,
    below), omitted lines (`compare::PlanOmission`, under `compare_view.rs`) and withheld lines
    (`ddl::Withheld`, below) — the modal's whole body but the SQL and the server's words — nor the
    compare tab's standing notes above its tree, nor an agent CLI's grade notice, pre-spawn
    refusal and failure fallbacks (`schemaic-ai`'s `SpawnRefusal`/`CliFailure`, below), though what
    the CLI itself said still is, nor Seed rows' unusable reply (`seed::SeedError`, below), nor
    the grid's Copy as SQL refusals, paste reports and row-panel refusals (`edit::CopySqlRefusal`,
    `edit::paste_report`, `rowjson::FieldRefusal`, under *Data grid*), nor the result diff's key
    refusals (`result_diff::DiffError`, under `result_diff_view.rs`), nor the find bar's bad
    pattern (`text_ops::PatternError`, under `editor_pane.rs`), though the regex crate's reason
    inside it still is, nor the hover's quick info (`quickinfo::QuickInfo`, under `editor_pane.rs`),
    though the schema's own names, types, defaults and comments inside it still are, nor Ctrl+K's
    diff-bar summary (`inline_plan_summary_text`, under `editor_pane.rs`), nor the autocomplete
    popup's own row words (`rank::RowWords`, under `completion.rs`), though a CTE row's *CTE*,
    an SQL term, still is.
  - **Four checks read the catalogue where the compiler cannot reach it.**
    `widgets-spinner-verbs` is a comma-separated list each language writes for itself, as many
    verbs as it likes, and `the_reserved_loader_width_fits_every_verb_and_dot_state` measures every
    verb in every catalogue against the slot Ctrl+K reserves for the loader;
    `error_bar_fits_explain` measures the error modal's own `error-modal-explain` and
    `error-modal-ai-fix` labels, so a longer language drops *Explain* where it actually crowds;
    `overlays::menu_order_gate` keys its skeleton on message ids rather than words, so it holds in
    every language (under *Popup menus*); and
    `source_gate::no_menu_label_ends_in_an_ellipsis` reads a `tr!` label's text in every catalogue,
    since a translation that adds the dots is the same habit in another language.
  - **Nothing decides behaviour by an English label** — decide where the thing is built, or give
    it an id. A comparison against a literal simply turns false once the label is translated, with
    nothing to say so: the editor's context menu built *Plan* on every engine and then removed it
    by `label == "Plan"` where the engine has no plan, and it is now built only when
    `plan::supports_plan` answers yes. `source_gate::nothing_is_found_by_its_english_label` fails
    on `label == "`, `label != "`, `label.as_str() == "` and `label.starts_with("`/`ends_with("` in
    both view crates' production code, and was red on exactly that site before the fix. It sees
    those spellings only — a label matched under another name, or through a `match`, passes it.
  - **Where and how a message is written.** `crates/schemaic-ui/i18n/en-GB/schemaic.ftl` is the
    source, in British spelling, and a new message goes there first, since `fl!` checks ids and
    argument names against that file alone. Ids are kebab-case and start with the area they belong
    to (`settings-language-restart`). A translation lives beside it at `i18n/<tag>/schemaic.ftl` and falls back to `en-GB` message
    by message. British English is the base on purpose — the spelling rule in `AGENTS.md` is the
    source catalogue's — and an `en-US` catalogue is possible but not planned.
    **The translations' conventions** — all eight machine-drafted and unreviewed, each header
    saying so. They shipped that way, and proofreading waits until the project has users who can
    contribute corrections: accepted, not a blocker. German was drafted first, being a large
    developer market and the longest-running of the common interface languages, which made it the
    layout stress test (*Layout for longer text*, below). The address is German
    *du*, Italian *tu*, Spanish *tú*, Romanian *tu*, Dutch *je*, French *vous*, Bulgarian *вие*
    and Albanian *ju*; quotation marks are the language's own — French `« »` with no-break spaces
    inside, Italian and Spanish `« »`, Dutch `‘ ’`, Romanian `„ ”`, Bulgarian `„ “`, Albanian
    `“ ”`; *AI* becomes *KI*, *IA* or *ИИ* where the language says so (German; Italian, French and
    Spanish; Bulgarian) and stays *AI* in Dutch, Romanian and Albanian. What stays English is what
    stays English in every language (above), plus *Verify CA*/*Verify full*, the plan modal's
    *Analyze* and the Generate submenu's `CREATE`/`INSERT`/`UPDATE`/`DELETE`, which name SQL. A
    translation may select on a plural English does not — the `erd-chip-*` counts are bare
    `{ $count } tables` in English and a `[one]`/`*[other]` select in every translation —
    which `a_translation_reads_only_the_variables_english_is_given` permits, since the select reads
    no variable English is not given. **Pass `tr!` an owned value, never a dereference**: `fl!` pastes each argument
    expression in front of `.into()`, so `n = *n` parses as `*(n.into())` and fails as E0282,
    "type annotations needed", pointing into `i18n.rs` rather than at the call. Bind the field by
    value in the `match` instead, as `rows_read_text` does. **Write a variant's leading space as
    `{" "}`, and a variant that says nothing as `{""}`**: Fluent trims the whitespace a variant
    starts with, so `[other]  for { $held }` would glue *for* to the word before it, and an empty
    variant needs something to stand in it — `activity-lock-wait-sentence` has both.
  - **Figures in the interface's marks** — phase 7 of *Interface translation*, landed: a
    figure the GUI shows is written with the interface language's decimal and grouping marks
    (`1,234.5` in `en-GB`, `1.234,5` in `de`), and **core's printers stay English**. The CLI, the
    MCP server, the AI's prompts, the clipboard's Markdown (`to_markdown`), a selection figure's
    copy (`Figure::copy`) and every pinned English test read the bytes they read before; the view
    marks what it shows. `text::NumberMarks { decimal, group }` is the pair, `NumberMarks::ENGLISH`
    core's own, and `locale::UiLanguage::number_marks` answers it per language.
    **A table, not CLDR (decided 2026-10-20).** The need is two characters per shipped language,
    which a table answers and `german_swaps_both_marks_and_english_keeps_core_s` pins. Every
    translation writes a decimal comma and they part on the group mark: `.` for German, Italian,
    Dutch, Spanish and Romanian, U+202F (the narrow no-break space) for French, and U+00A0 for
    Bulgarian and Albanian — a space either way that never breaks a figure across lines
    (`each_translation_writes_its_own_group_mark_after_a_decimal_comma`).
    CLDR data would move byte-pinned English under a `cargo update` — French's grouping space
    changed from U+00A0 to U+202F between CLDR releases, and the table takes the current one as a
    fixed choice that a release cannot move — and the shortcut through the catalogue
    does not exist: fluent-rs's `NUMBER()` ignores the locale, and a `FluentNumber` renders
    `f64::to_string`, with no grouping and always `.`. icu4x would also add a second
    locale-identifier stack beside `unic-langid`. What would reopen it: locale dates, collation, or
    a grouping no table expresses (Indian `1,23,456` — the grouper works in threes, always).
    **`text::group_digits(digits, group)` is the one grouper.** Four copies of its loop lived in
    `stats::group_digits`, `format::group_number`, `aggregate::grouped` and the settings view's
    `thousands`, each with the comma written in, so a language's mark had four places to be
    missed. `stats::group_digits(u64)` survives as the English wrapper, `aggregate::grouped(s,
    marks)` (now `pub`) delegates, `format::group_number` and `thousands` are gone, and Settings'
    `row_limit_label` groups through the marks. `text::group_number(s, marks)` groups a plain
    rendered number — sign and fraction, the fraction never grouped — and answers `None` for
    anything else (an exponent, a letter, a number already grouped), so a caller shows the text as
    it came rather than half-grouping it. `text::remark(english, marks)` rewrites a figure core has
    already printed, and **only a `.` or `,` between two digits is a mark**: a full stop, a list's
    comma and an ellipsis pass untouched, so a short line of figures (`1.2s · ↑1.2k ↓340`) can go
    through whole (`remark_leaves_punctuation_that_is_not_between_digits`). A prose sentence
    should not — its words are the catalogue's, with the figure an argument.
    **The view's side is `ui::i18n`**: `number_marks()` (English's until `init`, and in every
    test), `figure(&str)` over `remark`, and `count(n)`, `bytes(n)` and
    `contrasting_bytes(a, b)` — `human_count`, `stats::format_bytes` and
    `format::contrasting_bytes` through it. **A duration is not a figure to remark**, because its
    units are words: `i18n::took(Took)` words `history::Took` from `took-millis`/`took-seconds`/
    `took-minutes` — `850 ms`, `12,3 s`, `2 min 05 s` in German, where remarking core's string had
    shown `1,2s` and `2m 05s` — with the seconds' `$s` arriving remarked and the minutes' zero-padded,
    and `i18n::elapsed(ms)` is `took(transcript::elapsed(ms))`
    (`run_times_read_in_the_interfaces_units_and_marks`, which holds English to `Took`'s `Display`
    for every unit). **A figure
    handed to `tr!` goes as a pre-formatted string while the raw number selects the plural**, the
    catalogue's existing `$n`/`$rows` split. A plain integer `{ $n }` in a message stays ungrouped,
    as core's English has it — an ungrouped integer is valid German too.
    **Where core composes the figure itself, it takes the marks** rather than having its output
    re-marked: `model::goto_row_index(input, total, marks)` and `goto_target(…, marks)` (the go-to
    box, which has to read back what the stats line prints — under `text.rs` and *Data grid*),
    `transcript::TurnStats::summary_with(marks, took)` for the AI footer's token counts (its
    duration worded by `took`, as `history::outcome_line_with(entry, took, rows)` words a history
    row's), `chart_draw::layout(…,
    figures, measure)` for the tick labels and a histogram's bin edges under the axis — never a
    series' labels, which are the label column's data (`only_a_histogram_s_edges_take_the_marks`)
    — and `format::apply(fmt, v, marks)`, *Grouped* and *Bytes* being how a value is shown. The
    chart's SVG and PNG carry the marks, the file being a picture of the chart on screen.
    `edit::GridCells` gained `marks`, so the find bar matches the digits painted. **Ctrl+C, the
    export and *Copy as* read the raw value and never meet them; *Copy formatted* carries the
    interface's, being the cell as painted; and an AI attachment, which takes the formatted text
    on purpose, takes it in English's** — `GridCells::attached` reads through a copy of itself
    with `NumberMarks::ENGLISH`, since what is sent to a model stays English
    (`an_attachment_writes_its_figures_in_english_whatever_the_grid_shows`).
    **The gate is `every_core_figure_a_view_shows_takes_the_interface_s_marks`** (`i18n.rs`): in
    both view crates' production code outside `i18n.rs`, a `::human_count`, `format_bytes`,
    `human_bytes`, `contrasting_bytes` or `group_digits` path
    segment — called, or passed as a function — must share its line with `figure(`,
    `number_marks()` or `remark(` (marks a caller passed in, as the Properties index line's
    `index_fact_in` takes them) unless it is `i18n::`'s, and none may be imported by a `use schemaic_core…`.
    Durations left the list (`format_duration`, and `elapsed_text`, now deleted) when they stopped
    being a marks question: a remarked one is still in English's units, and core's printer is
    `pub(crate)`, so the compiler refuses a view the call the gate would have looked for.
    It first missed `.map(stats::format_bytes)`, a printer passed rather than called, and was seen
    red on it once it matched whole path segments. A `figure(` ending the line above also counts,
    as `cargo fmt` splits one there (it did, on the AI panel's timer, which now calls
    `i18n::elapsed`); a call split further, or a printer under another name, is not seen.
    **Its floor is a fixture, not a count of offenders**:
    `the_figure_gate_s_matcher_sees_a_bare_printer_and_passes_a_marked_one` runs the matcher
    (`unmarked_printer_calls`) over a few lines with known answers, and the gate itself only
    requires that it read the tree. The `seen >= 5` it replaced sat exactly on the number of
    printer calls the tree held, so moving one onto `i18n::bytes`, which is the remedy the gate's
    own message recommends, failed it as "gone blind".
  - **A word core gives as a property of a typed value is drawn UI-side**, not translated from
    core's text — `i18n::object_kind`/`object_kind_group` for `ObjectKind::label`/`group_label`,
    `history_panel::bucket_title` for `history::Bucket::label`, `activity_panel::kill_label`
    for `activity::KillKind::label`, `chart_view::chart_kind_label` for `chart::ChartKind::label`,
    `compare_view::compare_kind_group`/`compare_kind_word`/`empty_rows_message` for
    `compare::CompareKind::group_label`/`label` and `EmptyRows::message`,
    `event_editor::event_status_label` for `schema::EventStatus::label`, and
    `users_view::principal_kind_label` for `users::PrincipalKind::label`, and
    `account_editor::grant_subject_label`/`grant_level_label` for `users::GrantSubject::label`/
    `GrantLevelKind::label`, and `table_designer::identity_wording`/`storage_label` for
    `ddl::identity_wording` and `ddl::IndexStorage::label`, and `settings::keyword_case_label`/
    `comma_style_label`/`statement_gap_label` for `sqlfmt::KeywordCase::label`/`CommaStyle::label`/
    `StatementGap::label`, and `i18n::environment_label` and `ai_data_label`/`ai_data_hint` for
    `connection::Environment::label` and `AiData::label`/`hint` (in `i18n.rs` because the form, the
    top bar's badge and Settings' `ai_data_line` all call them), and `connection_form`'s
    `ssl_mode_label`/`ssl_mode_description`, `ssh_auth_label` and `auth_mode_label` for
    `SslMode::label`/`description`, `SshAuth::label` and `AuthMode::label`, and
    `grid::column_format_label` for `format::ColumnFormat::label`, the *Format as* menu, and — the
    one from
    `schemaic-ai` rather
    than core — `settings::agent_label` for `harness::Agent::label`, whose harnesses keep their
    product names and only *Disabled* is drawn —
    with a test holding its English equal to core's for every variant
    (`english_names_every_object_kind_as_core_does`, `english_titles_every_band_as_core_does`,
    `english_names_every_kill_as_core_does`, `english_names_every_chart_kind_as_core_does`,
    `english_names_every_compare_kind_as_core_does`, `english_says_every_empty_reason_as_core_does`,
    `english_names_every_event_status_as_core_does`, `english_names_every_principal_kind_as_core_does`,
    `english_names_every_grant_subject_and_level_as_core_does`,
    `english_words_the_identity_toggle_as_core_does_on_every_engine`,
    `english_names_every_index_kind_as_core_does_and_each_apart`,
    `english_names_every_formatting_choice_as_core_does`,
    `english_names_every_environment_and_data_level_as_core_does`,
    `english_names_every_picker_choice_as_core_does`,
    `english_names_every_column_format_as_core_does`, which walks `ColumnFormat::MENU`,
    `english_names_every_agent_as_the_ai_crate_does`).
    Where the word is a phrase core composes rather than a label, core answers with a value or
    keeps the composition and lets the caller word the part that is language: `history::ago` hands
    back an `Ago` that `i18n::relative_time` says from `ago-*` (the history and snippet rows, the
    palette's history rows, a result chip's tooltip), and `history::outcome_line_with` keeps which
    facts and where the separators go while `i18n::outcome_line` supplies `outcome-rows`/
    `outcome-rows-capped` and words the duration through `i18n::took` —
    `english_says_ago_and_rows_as_core_does` holding both to core's English,
    the outcome half by comparing `i18n::outcome_line` with `history::outcome_line` over 0, 1, 100
    and capped rows, a failure, no duration and an unknown outcome. It once compared the messages
    with a copy of core's row words written in the test, which stayed green while core changed.
    **A typed message** — phases 4 and 5 of *Interface translation*, landed, the shape settled on
    review (2026-10-19). Prose composed below the views — in `schemaic-core`, `schemaic-db` and
    `schemaic-ai` — becomes a typed value, an enum per area whose fields are the message's
    arguments, and core's `message()`/`Display` stays the English the CLI, the MCP server, the logs
    and the AI read. The interface words it through a renderer with one `tr!` arm per variant,
    beside the code that shows it (`ui/i18n.rs` holding the ones used across files, below), and a
    test holds the renderer equal to the English over every variant. A list is joined UI-side.
    Where core's function takes only plain values there is no value to type: the renderer restates
    it over the same arguments, held by the same kind of test. A word that is a property of a typed
    value (`ObjectKind`, `ChartKind`, …) is drawn by the interface while core keeps its English
    (above); a core function that composes a line around one counted word takes the word from the
    caller; and a value with a server's or driver's text inside passes that text through as it
    came. **Plurals are whole Fluent messages selected on the raw count, never `text::plural`** —
    phase 5's rule, and done: no hand-rolled plural is left in what the view crates show (the
    app's two remaining calls, `ai.rs`'s schema outline and the script run's write-log detail,
    write English on purpose, as do the view crates' tests), and core's remaining `plural(` calls
    are English renderings. What follows is each case as it landed.
    `alias_rename::refusal_message` words `intel::AliasRenameRefusal` from `alias-rename-*`
    (`Conflict` takes the trimmed `$name`), and `i18n::param_error` (in `i18n.rs` rather than
    beside a view, since the app calls it) words `params::ParamError` — `params-missing` with
    `$names`, the `:a, :b` list joined UI-side as core's `Display` joins it, and
    `params-not-a-number` with `$name` and `$text` — for the app's run guard (`guarded_run`,
    `guarded_run_all`), in place of `e.to_string()`. `english_refuses_every_alias_rename_as_core_does`
    (in `alias_rename.rs`) and `english_says_every_parameter_error_as_core_does` (in `i18n.rs`)
    hold the English equal byte for byte.
    The third is `cli_install`, whose `Refusal`, `Report` and `RowHint` the app's
    `install_cli::refusal`/`report` word from `install-refuse-*`/`install-report-*` and
    `settings::cli_hint` from `install-hint-*`, held by `english_refuses_every_install_as_core_does`,
    `english_reports_every_install_as_core_does` and `english_hints_every_cli_row_as_core_does`.
    The grid words three more: `stats::RowsRead`, the toolbar's row segment, by `rows_read_text`
    from `grid-rows-read`/`-capped`/`-of-total`; `model::AttachScope`, the phrase inside both
    *Attach … to chat* entries, by `attach_scope_text` from `grid-attach-scope-rows`/`-columns`/
    `-rows-and-columns`; and the large-duplicate confirmation's question, which `clone_rows` takes
    from `duplicate_question` — `grid-duplicate-question`, `$rows` the count and `$n` the plural
    selector — rather than `edit::duplicate_prompt`. They are held by
    `english_reads_every_row_segment_as_core_does`, `english_scopes_every_attachment_as_core_does`
    and `english_asks_the_duplicate_question_as_core_does` (`core_words_tests`). Their figures
    arrive as core prints them — `human_count`, `RowCount::label`, `stats::group_digits` (made
    `pub` for this) — and reach the message through `i18n::figure`, in the interface's marks
    (*Figures in the interface's marks*, above); `rows_read_clause`,
    `attach_scope_label` and `duplicate_prompt` remain only as the English wrappers core's tests
    read.
    Three counts a view drew as core's English are values now, the English kept beside them:
    `erd::MatchCount` (`match_count`, `match_label` its `message()`), worded by `erd_view`'s
    `match_count_text`; `activity::LocksTally`, by `activity_panel`'s `locks_total_text` and
    `locks_waiting_text`, which select on `$n` and carry the `+` in `$shown` as
    `ActivitySummary::figure`'s count does; and `schema::PartitionLine` (`partition_facts`,
    `partition_lines` its English), by `properties::partition_line_text`, whose
    `props-partitions-more` replaced a hand-rolled `if more == 1`. Two plain-arguments lines went
    beside them — `compare_view::plan_subject_text` for `SchemaPlan::subject_in` and
    `copy_table_view::rows_line_text` for `copy_table::rows_change`, both in the DDL preview. The
    review after v0.35.0 found all five still drawing English under the claim, above, that every
    file had left it. `english_says_the_find_readout_as_core_does`,
    `english_counts_the_locks_as_core_does`, `english_says_every_partition_line_as_core_does`,
    `english_names_the_plan_s_subject_as_core_does` and `english_says_the_rows_line_as_core_does`
    hold the English.
    *Restore from file*'s refusal is the first that joins a list: `script::RestoreRefusal` keeps
    its named switches as `DatabaseSwitch` values rather than text, and the app's
    `restore_refusal_text` words each from `restore-switch`/`-unnamed` — `$how` being the SQL that
    moves the restore (`USE`, `\connect`, `CREATE DATABASE`), never translated — joins them with
    `", "` in code, and words the sentence from `restore-refused`/`-more`, held by
    `english_refuses_every_restore_as_core_does` (`app_tests`). The separator is the code's, not
    the catalogue's, so a language that joins a list otherwise cannot say so.
    The statement timeout is a span rather than a sentence: `persist::statement_timeout_span`
    answers a `TimeoutSpan` — `Off`, or `Hours`/`Minutes`/`Seconds` in the largest whole unit — whose
    `Display` is the English, `statement_timeout_label` its wrapper, and `i18n::statement_timeout`
    words it from `timeout-off`/`-hours`/`-minutes`/`-seconds`. It has three readers that must say
    the value alike — Settings' dropdown (`settings::statement_timeout_label`), the connection
    form's choice (`conn_statement_timeout_label`, with `connections-statement-timeout-same` for
    `None`) and the app's `timeout_message`, which quotes it back — two spellings of *15 minutes*
    being how the setting and the error come to disagree. `english_says_every_statement_timeout_as_core_does` and
    `english_names_every_timeout_choice_as_core_does` hold the English.
    Server Activity's state words are the case where a *search* has to follow the translation: the
    box matches the word a row shows, so translating only the rows would leave the translated word
    on screen and core's English the one that finds it. Core is handed the words rather than having
    its output translated, as `outcome_line_with` is — `activity::matches_query_with` takes them as
    a `Fn(SessionState) -> &str` — and the counts line takes `ActivitySummary::figure`, the count
    with the `+` a capped list earns, and words the noun from `activity-count-*`
    (`activity_panel.rs`), held by `english_counts_and_names_every_state_as_core_does`.
    A single-file export's notes are the case of a sentence whose clauses come and go:
    `export::export_said` answers an `ExportNote` and `i18n::export_note` words it — `export-note-rows`
    for the head, each `caveats()` entry from `export-note-withheld`/`-blanked`/`-cut` (`$limit`
    is `XLSX_MAX_CELL_CHARS` passed as a string), joined with `"; "` in code and placed after the
    head by `export-note-with-caveats` (`$head`, `$caveats`). The failure and cancel notes had no
    value to type, their inputs being plain already, so `i18n::export_failure_note` and
    `export_cancel_note` take the same arguments core's do (`export-failure-partial`,
    `export-cancelled-partial`/`-nothing`), and the grid words `SqlCopy::said` by `sql_copy_words`
    (`copy-sql-*`). The grid's destinations *the clipboard* and *the file*, a `$name` in those
    sentences, are `export-dest-clipboard`/`-file`. `english_says_every_export_note_as_core_does`
    (four row counts, all eight combinations of the three losses, one and two columns) and
    `english_says_every_failed_or_cancelled_export_as_core_does`, both under `ui/i18n.rs`, and
    `english_says_every_sql_copy_note_as_core_does` (`grid.rs`'s `core_words_tests`) hold the
    English. As with
    *Restore from file*, the separator is the code's.
    The Export modal's dump and folder notes are the same plain-arguments case, and their
    renderers sit beside their one caller rather than in `i18n.rs`: `dump_view.rs`'s
    `dump_done_note`, `dump_cancel_note`, `files_*_note` and `folder_replace_prompt` each take the
    arguments of the core function they mirror (`dump::done_note`/`cancel_note`/
    `folder_replace_prompt`, `export::files_*`) — those stay English, for the CLI's dump report and
    core's tests — and `english_says_every_dump_outcome_as_core_does` and
    `english_says_every_folder_outcome_as_core_does` hold the two equal. So is the desktop
    notification: the app's `notify::finished_note` takes `history::finished_note`'s arguments
    and words them from `notify-*` (under `notify.rs`).
    Their failures are the case of a typed message that has to travel before anyone words it:
    `dump::ExportFailure` is built in the writer threads and the read (`cli/dump.rs`), carried
    through `WriteEnd`, `DumpVerdict` and the two outcome enums, and worded only at the one view
    that shows it, `dump_view`'s `export_failure_text`, before `i18n::export_failure_note` or
    `files_failure_note` puts it in its sentence. A refusal the interface has already worded — the
    app's two before a run, when the connection cannot be had — rides the same field as
    `ExportFailure::Worded` and passes through untouched. `english_says_every_export_failure_as_core_does`
    holds every variant equal to the `Display` the CLI prints.
    The launchers' refusals are the case where the missing `Display` is the guard.
    `launch::Refusal` is the `Err` of every refusing function in `core::launch` and the answer of
    `AuthMode::external_client_refusal`, and the app's `refusal_text` (beside `Unreached`, in
    `main.rs`) words each variant from `cli-refused-*`. `mysql_shell_config`, `psql_shell_config`
    and `sqlite_shell_config` hand the `Refusal` up; what returns a `String` — the `*_shell`s,
    `mssql_shell_config`, `tool_tunnel_check`, the backup and restore builders, `reach_for_cli`,
    `open_db_cli`'s tunnel TLS message shell, and `conn_sources`' Docker import for its two (a
    refused `PATH` `docker`, `docker_inspect_args`) — maps through it. Core gives `message` and no
    `Display`, since no headless surface says these, so the `why.to_string()` that would stay
    English in every language does not compile. `english_says_every_launcher_refusal_as_core_does`
    (`app_tests`) holds `refusal_text` equal to `message` for every variant of `Refusal::ALL`.
    The startup notices are the case where the words are queued before there is anyone to say
    them. `persist::RECOVERIES` holds `persist::Notice` values — a file unreadable or missing, a
    keyring that would not answer, a save it did not fully take — and, like `Refusal`, `Notice`
    has `message` and no `Display` (under `core/persist.rs`). `i18n::notice_text` words each from
    `notice-unreadable`/`-kept`/`-swept`, `notice-missing`/`-restored`,
    `notice-keyring-unread`/`-why`/`-kept`, `notice-in-the-clear`/`-where` and
    `notice-undeleted`/`-after`, one message per line joined with `"\n"` and the save's two
    paragraphs with `"\n\n"`, as core's English breaks them; a parser's or keyring's own text
    inside goes in as written. `report_recoveries` and the app's mid-session save both show it,
    and `Notice::Said` is the one variant it passes through, its text already worded by the
    caller. `english_says_every_startup_notice_as_core_does` holds it equal to `message` for both
    `swept` states, both siblings, the keyring read with and without its reason and the save's
    refusals one way, the other and both. `Hydration::cli_notice` is the CLI's and stays English.
    The connection import's refusals are the case where the words carry a secret. `conn_import`'s
    `SkipReason` held finished English in `Unreadable(String)` and now carries values — `BadUrl`
    with the `UrlError`, `NotAnExport`, `NewerExport` with the version — and
    `i18n::skip_reason_shown` and `url_error_text` word them from
    `conn-import-reason-*`/`conn-import-url-*` (in `i18n.rs`, since the import modal and the
    app's paste field both say one). Since a parser's text inside a reason may be a
    password, the worded sentence goes through `conn_import::redact_shown` whole, as core's
    `message` does its English (under `connection_import.rs`);
    `english_says_every_import_refusal_as_core_does`, in `i18n.rs`'s tests, holds the two equal
    and fails without the redaction.
    The file import's refusals are the case where a library's text and Schemaic's share one
    channel. `ImportError::Read(String)` held both, and now holds only the library's — every
    sentence Schemaic writes itself is `ImportError::Refused(ReadRefusal)` (under `core/import.rs`).
    `i18n::read_refusal_text` words each `ReadRefusal` from `import-refuse-*`, and
    `import_error_text` the whole error — `import-read-failed` with `$error` the library's text as
    written or the worded refusal, `import-no-columns-mapped`, `import-preview-record-too-large` —
    both public for the app, whose probe, check and load show them. The modal's own two,
    `MemoryWarning` and `IssueTail`, it words beside their one caller (`import_view.rs`).
    `english_refuses_every_unreadable_file_as_core_does` holds every refusal equal to `message`
    bare and to `ImportError`'s `Display` inside one.
    The plan modal's warnings are the case where the word for nothing differs by engine.
    `PlanWarning` held its sentence as a `String` and now holds a `PlanNote`, with SQL Server's
    own warnings a `ServerWarning` inside it (under `core/plan.rs`); `plan_view`'s `plan_note_text`
    and `server_warning_text` word them from `plan-*`. A table the plan does not name is `None`,
    and its sentence a message of its own — `-result`, since MySQL's English says *the result*,
    and `-unnamed` where PostgreSQL's and SQL Server's say *a table* — not a word handed in as
    `$table`. `english_says_every_plan_note_as_core_does` holds the English. SQL Server's plan table
    itself — its headings, *Statement N* rows and could-not-read row — went the same way without
    leaving its cells: core still writes them in English for the AI and the CLI, and says which
    they are in `QueryPlan::wording`, a `PlanWording`; `plan_view`'s `shown_table` words exactly
    those from `plan-ms-*` (`english_shows_a_plans_table_as_core_wrote_it`).
    The run guard's reasons are the case where a front end used to match the text. `RunVerdict`
    held finished English and now holds a `sql::GuardReason`, the destructive statements' warnings
    an `Unsafe` inside it (under `core::sql`), and `i18n::guard_reason_text` words it from
    `guard-*`, its private `unsafe_text` each `Unsafe`. `DATABASE` and `SCHEMA` get a message each
    (`guard-drop-database`/`-schema`, `guard-replace-database`/`-schema`) rather than core's
    lowercased keyword spliced into one sentence; `$kind` (`DELETE`/`UPDATE`) and `$obj` (`TYPE`,
    `DOMAIN`, `EXTENSION`) are SQL and go in as spelt. The staged-edits question is `guard-staged`,
    a plural on `$n`, and after another reason the two are joined with a space in code, as core's
    English joins them. The app's `guarded_run`/`guarded_run_all` set `RunGuard.message` from it,
    and `ScriptRequest::approved`'s `Err` is it — only ever a `Block` there, since the script panel
    answers the `Confirm`. `ReadOnlyRefused`'s `$reason` is a `sql::ReadOnlyReason`, which the
    private `read_only_reason_text` words from `read-only-*` — `$heads` joined with `/` as core joins
    them, `$word`, `$name` and `$clause` SQL and spelt as the statement had them — so the guard bar
    on a read-only connection, where the editor asks that gate of every statement on every engine
    (it once asked only on SQL Server, `read_only_is_a_rollback`; the write-guard invariant has
    why), reads wholly in the interface's language; it used to splice
    `read_only_reason`'s English into a translated sentence.
    `english_says_every_read_only_reason_as_core_does` holds each reason equal to `message`, bare
    and inside `ReadOnlyRefused`, over every engine's heads. The CLI keeps core's `message`, and
    its `refused_for_no_database` now matches the `NoDatabase` variant (`cli/run.rs`).
    `english_says_every_guard_reason_as_core_does` holds every reason equal to `message`, each
    keyword an `Unsafe` carries, and the staged question alone and after another over one change
    and three. What the guard decides did not move — only how its reason is carried.
    The grid filter's refusals are the file import's case again, smaller. `FilterError::BadCondition`
    held `sqlparser`'s text and two sentences of Schemaic's built at their sites — `build_query`'s
    doubled-bracket refusal and `parse_condition`'s trailing input — and now holds only the
    parser's, the two being `BracketInName` and `TrailingInput` (under `core::filter`), with
    `message` the English and no `Display`. `grid.rs`'s `filter_error_text` words them from
    `grid-filter-bracket-in-name`/`-trailing-input` and passes the parser's text through, for the
    filter apply; `english_refuses_every_filter_as_core_does` (`core_words_tests`) holds it equal
    to `message` for all three.
    The account browser's notes are the case where the English was not all core's to begin with.
    `Grants::note` and `Principals::note` held finished English — from core's `*_note` functions,
    and from `schemaic-db` itself: SQL Server's sentences joined inline in `mssql.rs`, PostgreSQL's
    *this cluster* passed in by `pg.rs` — and now hold a `users::GrantsNote`/`PrincipalsNote` (under
    `core::users`), with `message()` the English and no `Display`. `users_view.rs` words them from
    `users-note-*`, joining sentences with a space in code as core does, held by
    `english_says_every_users_note_as_core_does`; the live tests read `message()`.
    Its attributes are the case of a value that was already a pair of English strings rather than
    a sentence: `Principal::attributes` held `(label, value)` text from all three folds and now
    holds `users::AccountAttribute`s, `label()`/`value()` the old pair. `users_view.rs`'s
    `attribute_text` words both halves from `users-attr-*` and `users-login-type-*`, the server's
    text inside passing through, held by `english_words_every_account_attribute_as_core_does`.
    The object editors' refusals are the case where core's English has more readers than the
    interface does, and every editor — database, view, event, object, routine, trigger and the
    table designer — now takes the shape below. A draft's `validate` stays core's English — still
    a `Vec<String>`, because core's many tests (and the live ones) read it — and becomes a thin
    wrapper over a typed
    `problems()`, one enum per draft — or per editor, where one editor holds several drafts —
    whose `message()` is that English byte for byte and which has
    no `Display`. The editor words `problems()` from the catalogue and asks
    `problems().is_empty()` for Preview's readiness, so no editor reads `validate` any more. `DatabaseDraft::problems` answers `DatabaseProblem` (`NoName`, `BadCollation`),
    `ViewDraft::problems(dialect)` answers `ViewProblem` (`NoName`, `NoSelect`, `NotAQuery`,
    `Materialized`, `Hidden`, `Verbatim`) — `view_not_editable_reason` now answering
    `Option<ViewProblem>` (`Hidden`/`Verbatim`) where it answered a `&'static str`, while
    `view_is_editable` still asks only `is_none()` — and `EventDraft::problems` answers
    `EventProblem` (`NoName`, `NoTime`, `NoInterval`, `NoUnit`, `NoBody`). The object editor's
    four drafts share one flat `ObjectProblem`, answered by `EnumDraft`, `DomainDraft`,
    `SequenceDraft` and `TsqlObjectDraft::problems` and dispatched by `ObjectDraft::problems`;
    its figures are carried as each draft holds them — a PostgreSQL sequence's as `i64`, SQL
    Server's `StartedOutside` start as the text typed — and SQL Server's not-a-whole-number
    refusal is one `NotWhole { field: SequenceField }`, which the interface words as a whole
    message per field. `RoutineDraft::problems(dialect)` answers `RoutineProblem` (`NoName`,
    `NoLanguage` and `NoBody`, each carrying the `RoutineKind` its sentence names; `NoReturnType`,
    `ProcedureReturns`, `BareParameter`, `RefusedOption { shape, option, native }` with the option
    as its SQL, `NativeNeedsSchemaBinding`, `InlineWithExecuteAs`, `NumberedRename { was, group }`
    with the group's names already joined, `TriggerFunctionArgs`). The trigger editor's two drafts
    share one `TriggerProblem`, answered by `TriggerDraft::problems(dialect, host)` and
    `TriggerSetDraft::problems(current, dialect, host)`, the per-engine arms and their order
    unchanged: an engine's own refusals are its own variants (`PgNoFunction`, `MsNoBefore`,
    `MyNoView`, …, with `InsteadOfViewOnly` and `UpdateOfWithoutUpdate` shared by PostgreSQL and
    SQLite), and a refusal several engines make in the same words carries the product name as
    `engine: &'static str` (`OneEvent`, `NoTruncate`, `NoBody`, `RunsBody`, `NoWhen`); the set's
    own are `ConstraintTrigger`, `NotEditable` and `Twice`, each with the `name`, and `RankClash
    { rank, event, head, last }`, which keeps the names before the last as a list — the interface
    joins them, and counts them for *both* or *all*. `TableDraft::problems(dialect)` answers
    `TableProblem`, a variant per refusal carrying the draft's own names (`index`, `column`, `fk`,
    a check's `name`, an XML index's `parent`; `StrictType` the `column` and its `data_type`), SQL
    Server's XML, spatial and columnstore rules among them. Its `ClusterClash { key, indexes }`
    keeps what clusters as a flag and a list, because core's English builds the sentence's subject
    from them (*the primary key and index a and index b*): the interface builds the same from
    `table-problem-cluster-key` and `-cluster-index`, joins them in code with a spaced
    `table-problem-cluster-and`, and hands the result to `table-problem-cluster-clash` as
    `$clustered`. The event's name clash
    is not a problem but a decision of its own: `EventDraft::clashes` is it, `name_clash` the
    English `Option<String>` wrapped round it, and the editor words a clash from
    `event-name-clash` with `$name` the draft's trimmed name. The routine's is the same shape with
    more to say: `RoutineDraft::clash` answers a `RoutineClash` (`kind`, `name`, `whole` — the
    plan runs as one transaction, so the server refuses it and nothing changes — and the
    `original` a rename would destroy, `None` for a new routine, which the English calls *the
    original*), with `name_clash` its English and the decision itself unchanged. Each editor's
    renderer sits beside it (`database_problem_text`, `view_problem_text`,
    `event_problem_text`/`event_clash_text`, `object_problem_text`,
    `routine_problem_text`/`routine_clash_text`, `trigger_problem_text`, `table_problem_text`),
    held by
    `english_says_every_database_problem_as_core_does`,
    `english_says_every_view_problem_as_core_does`,
    `english_says_every_event_problem_as_core_does`,
    `english_says_every_object_problem_as_core_does`,
    `english_says_every_routine_problem_as_core_does`,
    `english_says_every_trigger_problem_as_core_does` and
    `english_says_every_table_problem_as_core_does` (the clustered clash with and without the key,
    over one index and two). Those tests walk a hand-written list of
    variants rather than an `ALL`, so a new variant fails the renderer's exhaustive `match` at
    compile time but is not held to its English until someone adds it to the list.
    The DDL preview's change list is the case of one value with many small parts. `Change::summary`
    built finished English and is now `self.line().message()`: `Change::line` answers a
    `ddl::ChangeLine`, one variant per sentence the list says, its fields the sentence's arguments
    and its lists carried whole, joined by whoever words them; `message()` is the English byte for
    byte, which the CLI, the MCP server and every core test still read
    (under `ddl.rs`). The parts are typed alongside it — `ClauseKind` with `AddedClause` and
    `ClauseChange` for a column's free-SQL clauses, `MoveTo`, `IndexLine`, `TableOptionEdit`,
    `SequenceEdit`, `EventEdit` and `PrivilegeWords` — and a routine's kind is an `ObjectKind`
    (`routine_object_kind`), so the interface names it through `i18n::object_kind`. `ClauseKind::label`
    (*default*, *generated as*, *on update*, *collate*) is SQL, and both languages print it
    untranslated. `ddl_preview.rs`'s `change_line_text` words a line from `ddl-line-*`,
    `ddl-clause-*`, `ddl-index-*`, `ddl-table-option-*`, `ddl-event-edit-*`,
    `ddl-sequence-edit-*` and `ddl-privileges-*`, and `plan_line_text` puts a comparison's line
    after its object with `ddl-line-of`; `preview_of` and `preview_of_plan` fill
    `DdlPreview::changes` from them, and `preview_change_warning`'s `reword` now takes and returns
    `ChangeLine`s, worded after it (under `ddl_preview.rs`). **The added-index line is one
    message**, `ddl-line-add-index`, with the selector rule below applied to every optional part —
    `$unique`, then `$has_kind`/`$kind` (a `ddl-index-kind-*` word), `$has_method`/`$method`,
    `$has_columns`/`$columns`, `$has_include`/`$include`, `$has_order`/`$order` and
    `$has_predicate`/`$predicate`, each leading space a `{" "}`. **A part's presence is a yes/no
    flag beside its value, not the lock-wait sentence's `none` sentinel**, because these values
    are the user's names: a key column or method literally called `none` would select the empty
    variant and vanish from the line, so the test words an index whose method is `no` and whose
    key is `none`. A column line's clauses are a list like any other: each `ddl-clause-added` or
    `ddl-clause-changed` is worded on its own, joined with `", "` in code and handed to
    `ddl-line-add-column`, `-rename-column`, `-retype-column` or `-change-column` as `$clauses`
    beside `$has_clauses`; a rename's type change is a selector in its message (`$retyped`,
    `$old_type`, `$new_type`). The joins left in code — clauses, table options, sequence and event
    edits, column lists — are lists, joined before they are handed in, as *Restore from file*'s
    are. `english_says_every_change_line_as_core_does` holds `change_line_text` equal to
    `message()` over every variant — optional parts present and absent, counts one and many, lists
    empty, single and several, every `IndexStorage`, every object and principal kind — and
    `plan_line_text` equal to `SchemaPlan::summaries`' line.
    The risk block followed by the same route. `Change::risks` is `risk_lines(dialect)`'s English
    and `ChangeSet::destructive` `risk_lines()`'s: a `ddl::Risk` per sentence, its fields the
    arguments, `Risk::message()` the English byte for byte (under `ddl.rs`). The parts are typed
    beside it — `ViewCost` (a `ViewCostEngine` and `restored`, what goes with a dropped view and
    whether its `INSTEAD OF` triggers come back, which `view_drop_cost` now returns rather than a
    sentence), `RecreateViewWhy`, `TypeSubject` (a column or a domain), `SignedKind`,
    `RecreateKind` and `ObjectDrop` — and the privilege lists are the change lines' own
    `PrivilegeWords`, which `privilege_words_text` words for both renderers. A comparison's risk is
    a `compare::PlanRisk` — `Of { subject, risk }`, `Cycle`, or `Clash(ClashNote)` — with
    `SchemaPlan::destructive()` its English. `risk_text` and `plan_risk_text` word them from
    `ddl-risk-*`, each optional clause a selector inside one message rather than a sentence glued
    on in code: `ddl-risk-revoke-privileges` carries `$cascades`, `$deny` and
    `$has_cleared`/`$cleared`, and `ddl-risk-recreate` `$has_columns`/`$columns` and `$more`.
    `english_says_every_risk_as_core_does` holds both equal to `message()` over a hand-written list
    — every variant, each engine's view cost restored and not, optional parts both ways, the cycle
    and every `ClashWhy` under every `ClashCost` — so, as above, a new variant fails the `match` at compile time but is held to
    its English only once someone adds it.
    The *Used by* lines went the same way, as two values sharing one part. A drop's line is
    `used_by::drop_listing`'s `Listing { kinds, names, more }`, `drop_warning` its English; a
    rename's or column edit's is a `UsedByNote { subject, listing, then }` from `change_note` and
    `change_notes`, `change_warning` and `change_warnings` their English (under `used_by.rs`).
    `listing_text` words a `Listing` from `ddl-used-by-*`: each kind counted by its own plural
    message (`ddl-used-by-count-view` … `-expression`), all but the last joined with `", "` in code
    and `ddl-used-by-counts` putting *and* before the last, then `ddl-used-by-listing` with the
    names and a `$more` selector whose `0` variant says nothing. `used_by_drop_text` sets it in
    `ddl-used-by-drop`; `used_by_note_text` sets it in `ddl-used-by-change` beside the subject —
    the object's name, or `ddl-used-by-column` — and a `$then` selector (`renamed`, with `$name`
    the old name, `dropped`, `retyped`). `with_used_by` fills `DdlPreview::used_by` with the
    latter and the schema tree's Drop appends the former (under `ddl_preview.rs`).
    `english_says_every_used_by_line_as_core_does` holds both equal to core's English over every
    kind at one and many, one kind and several, with and without more, and each subject under each
    consequence.
    The after-commit refresh note — what SQL Server's walk of other databases' `SELECT *` views
    could not do once a plan committed (under `ddl.rs`) — went the same way. `ddl::elsewhere_notes`
    returns an `ElsewhereNote` per sentence: `Failed { table, error }` for a batch that failed
    whole, `Unrefreshed { table, views }` and `Unsearched { table, databases }`, each list a
    `Capped { shown, more }`, the first eight items and how many more, whose `message()` adds
    *, and N more*. `ElsewhereNote::message()` is the English and `elsewhere_report` the notes'
    English joined by newlines, unchanged byte for byte. `elsewhere_note_text` words them from
    `ddl-elsewhere-*` — `$count` plurals, the list set in `ddl-elsewhere-list` with a `$more`
    selector — the items inside, `db.module (error)` or a database, the server's words as they
    came. The app's `run_ddl` words each note of each batch and joins them with `"\n"` as
    `elsewhere_report` does, and a failed batch's error is now `db_error_shown`'s where it was
    `to_string()` — which it may be, since the note is `DdlOutcome::Applied(Some(note))`, for the
    eye, and no log keeps it. `english_says_every_refresh_note_as_core_does` holds the worded lines,
    joined, equal to `elsewhere_report` over the batch's failure, one, three and eleven views and
    databases (with and without an error), and one of each. The omitted notes went too, worded by
    `compare_view::omission_text` from `compare::PlanOmission` (under `compare_view.rs`), and with
    them the compare tab's standing notes, which are the same values. The withheld lines were the
    last of it, and with them nothing in the modal's body is core's English but the SQL and what
    the server says. `ChangeSet::withheld_lines()` returns a `ddl::Withheld` per sentence:
    `Change(ChangeLine)` for a change the engine has no statement for, said as its change line
    (where `unsupported` used to read `Change::summary`); `UnreplayableRename`, `NumberedGroup` and
    `LossyViewIndexes { created }`; SQL Server's rebuild refusals `TsqlLossyIndex`,
    `TsqlEncryptedTrigger`, `TsqlSignedTrigger` and `TsqlRenamedTrigger`; and SQLite's —
    `PartlyRead { index, why }`, its `PartlyReadWhy` one of `MayName(Moved)`, `NotFound`, `Edited`
    and `NoText`, then `ConditionNamesDropped`, `StrandsTriggers`, `StrandsOtherTrigger { trigger }`
    (`Some((name, table))` where the trigger's text could be read), `StrandsGeneratedColumn`,
    `CannotRestate { clauses }` and `VirtualTable`. A moved column is a `Moved`, which replaced
    `MovedColumn::phrase`, and `unrestatable_sqlite_clauses` returns `SqliteClause`s
    (`Deferrable`, `OnConflict`, `DescKey`, `CollateKey`) whose `label()` is the English; every
    producer returns these values, `IndexReplay::Refuse` carrying one. `Withheld::message()` is the
    English byte for byte and `unsupported()` that list's English, which stays what
    `withheld_header` (the script's `-- INCOMPLETE` header, so Copy and Open in editor), the CLI
    and the tests read. A comparison's is `SchemaPlan::withheld_lines()`, each paired with its
    subject, `unsupported()` its English as *subject — line*. `withheld_text` words a `Withheld`
    from `ddl-withheld-*` — `$count` plurals for the triggers and indexes, `-view-index-create` and
    `-edit` for `created`, a `$why` selector for the partly-read reason — a `Moved` through
    `moved_text` (`ddl-moved-renamed`/`-dropped`), each clause from `ddl-sqlite-clause-*` with the
    last joined by `ddl-sqlite-clauses`, and a refused change through `change_line_text`;
    `plan_withheld_text` sets that in `ddl-withheld-of` beside the subject. `preview_of` and
    `preview_of_plan` fill `DdlPreview::withheld` from them, and its emptiness still gates Apply.
    `english_says_every_withheld_line_as_core_does` holds both equal to core's English over every
    variant, each `Moved` under every strand and the partly-read `MayName`, every other reason, one
    and several triggers and indexes, a view created and edited, and one to four clauses.
    An AI proposal's refusal is the same two-reader case, its readers on either side of the chat.
    `ProposeError` held two pieces it had already worded in English: `Invalid` carried
    `validate`'s first sentence, and `NotAnExpression`'s `what` the field's name as a `String`.
    `Invalid` now carries the `ddl::TableProblem` itself — the first of `TableDraft::problems` —
    and `what` is an `ExpressionField` (`Default`, `Generated`, `Check`) whose `label()` is the name
    the English gives it; the `Display` is unchanged, and it is what `propose_table_change` hands
    the model, so the model reads English whatever the interface's language. The card reads
    `propose_error_text` (under `ddl_preview.rs`), which words every variant from `propose-*`, the
    `Invalid` problem through the designer's `table_problem_text` — `pub(crate)` for it — and the
    field as a `$field` selector whose every arm is a whole sentence; `preview_proposal`'s own two
    refusals and the panel's two scope refusals (`propose-no-database`, `propose-other-connection`,
    in `ai_panel.rs`) are catalogue messages beside them.
    `english_refuses_every_proposal_as_core_does` holds the English equal to the `Display` over
    every variant, each `ExpressionField` and two `TableProblem`s.
    An agent CLI's refusals came from `schemaic-ai` as finished English, and reached the AI panel,
    Ctrl+K and the grid's Fill and Seed that way whatever the interface's language. They are values
    now (under `schemaic-ai`'s `lib.rs`), and four renderers here word them: `spawn_refusal_text`
    for a `SpawnRefusal` — `ai-oversize` with the unit from `ai-unit-*` and the closing sentence
    from `ai-levers-*`, and `ai-batch-shim`; `cli_failure_text` for a `CliFailure` —
    `ai-cli-exited`/`-signalled`, a `Said` passing through untouched; `seed_error_text` for core's
    `seed::SeedError` (`ai-seed-*`, the JSON parser's text a `$error`); and
    `constraint_notice_text` for `Constraint::notice`, which kept its plain arguments. **That one
    re-asks the harness rather than reading a value**: `ai-constraint-sandboxed`,
    `-workspace-rules`, `-denylist`, `-unrestricted` and `-unknown` are chosen by the same
    `restricted_means_sandbox`/`restricts_by_workspace_rules`/`seals_by_flag` order `notice` uses,
    and the first two carry an `$mcp` selector (`isolated`/`shared`) for the sentence `notice`
    appends where `isolates_mcp_servers` is false — so the dispatch exists twice, and the test is
    what keeps the two orders one. The figures go in as `Display` writes them, ungrouped, through `figure` for the interface's marks.
    Every call site that shows one takes the renderer — `inline_plan`'s refusal, `run_inline`'s
    failure, both session models' spawn refusals and their turn- and session-ended errors,
    `cursor::approve_verdict`, the app's own `spawn_refusal` gate, Settings' grade notice and the
    Seed rows parse error — and none of these is sent to a model.
    `english_says_every_agent_refusal_as_schemaic_ai_does` holds the English equal over every
    `Harness` × `Constraint`, both units × both levers, a shim, every `CliFailure` and every
    `SeedError`; it was seen red with *also* dropped from the en-GB sandboxed message.
    The editor's diagnostics are the case where one value has two readers in two languages.
    `intel::Diagnostic` held a finished `message: String` and now holds `what`, an
    `intel::DiagMessage` (under `core::intel`), with `Diagnostic::message()` the English byte for
    byte and no `Display`. The squiggle's hover words it — `editor_pane.rs`'s `tip_message`
    through `diag_message_text` and `flow_error_text`, from `diag-*` and `diag-flow-*` — while
    `problems_in_range` hands the AI fix prompt `message()`, so the model reads English whatever
    the interface's language. `Syntax`'s parser detail passes through as it came; `Server` holds
    its English `text` for the prompt and the interface's `shown` for the hover
    (`Diagnostic::shown_as`, from `db_error_shown` or a `ModalError`'s `displayed()`), the client's
    frame worded and the server's message inside it untouched.
    `english_says_every_diagnostic_as_core_does` holds the two equal, over a hand-written
    list like the editors' above.
    A write path's failure is the case where the value had to be carried through a layer that only
    knew strings. The grid's Commit, an import and a copy failed with a `DbError::Query` or
    `Refused` whose text `schemaic-db` had already finished — the cause, `Rollback::note` appended,
    "query failed:" in front — and now fail with `DbError::Write(WriteFailure)` (under
    `core::model` and `schemaic-db`'s `lib.rs`), whose `Display` is that same English.
    `i18n::write_failure_text` words it from `write-*`: the 1-row miss a message per `WriteAction`
    (`write-one-row-delete`/`-update`/`-insert`), the batch count `write-batch-inserted`/`-affected`,
    the two Stops, each `Rollback` its own `write-rollback-*`, and `db-query-failed`
    (`write-query-failed` until a `DbError::Query` came to be worded with it too) wrapping
    the lot exactly when `WriteCause::is_statements` says core's English would. A `Row` is the
    private `row_error_text`'s: a record's `Issue` is `write-row-issue`, or `write-row-issue-empty`
    for a blank field, with what is wrong from `issue_kind_message` — the import modal's own words
    for the problem list, moved here from `import_view.rs` so the two cannot drift — and each
    `CopyStop` its `write-copy-*`, a message apiece for a column named and unnamed. **The three
    rollback messages open with `{" "}`**, because Fluent drops a value's leading blank and the
    clause is appended straight after the cause; written as plain text they lose the space.
    The app's `db_error_shown` asks it for a `Write`, and words every other `DbError` variant
    itself from the section *A database error's own words* — `Query` and `QueryAt` as
    `db-query-failed` (`i18n::db_query_failed`), `Connect` as `db-connection-failed`, `Cancelled`
    as `db-query-cancelled` — with the server's or driver's message inside as it came — a
    `QueryFailed` as `db-query-failed` around `i18n::query_failure_text`, which words its
    `QueryFailure` from `db-query-*`, SQL Server's batch note and SQLite's lock advice each a
    message of its own joined under the server's or SQLite's text with line breaks in code (one
    and two, as core's English breaks them), a `ConnectFailed` as `db-connection-failed` around `i18n::connect_failure_text`, which words its
    `ConnectFailure` from `db-connect-*`, `db-ssh-*`, `db-tls-*` and — through the private
    `sign_in_failure_text`, the CLI's product name passing through — `db-sign-in-*` (a route
    refusal through `connection_form.rs`'s `route_note_text`, now `pub(crate)` for it), and a
    `Refused` through `i18n::db_refusal_text`. That variant is already the whole sentence (under
    `schemaic-db`'s `lib.rs`), so it used to pass through as written, which was English whatever
    the interface's language; it carries a `DbRefusal` now (under `core::model`), and
    `db_refusal_text` words each variant from `db-refused-*` — `Transport` a message per cloud
    sign-in and `db-refused-transport` for the rest, `InitSql` through `connection_form.rs`'s
    `init_refusal_text`, `ReadOnly` through `read_only_reason_text`, the SQLite size cap's two
    sizes through `bytes`, so in the interface's marks — with the names, types and values inside
    passing through. `write_failure_text` words a `WriteCause::Refused` with the same function.
    `english_says_every_db_refusal_as_core_does` holds it equal to `message` over every variant,
    each sign-in's transport, and the blank and code-page refusals with and without a row and
    the code page's lost character with and without one; it was taken red by rewording en-GB's
    code-page *has no character for* clause. `english_says_every_connect_failure_as_core_does`
    does the same for `connect_failure_text` over every variant, both `SshPhase`s, a changed host
    key with a recorded key and without one, a route refusal, and every `SignInFailure` for every
    CLI, a host and a user each with all three `ValueProblem`s; it was taken red by removing the
    alignment space in en-GB's `Offered  { $key }` (under `ssh.rs`), and again for the sign-in
    arm by shortening en-GB's `db-sign-in-no-token-from-azure-silent`.
    `english_says_every_query_failure_as_core_does` does the same for `query_failure_text` over
    every variant; it was taken red by shortening en-GB's `db-query-no-import-columns`.
    `modal_error_shown` is `modal_about_another_statement` *shown as* that text
    (`ModalError::shown_as`), and `db_error_pair` is `(to_string(), db_error_shown)` for a caller
    that raises its error as text. The Commit's `CommitDone::Failed`, a Manual tab's `end_tx`, a
    failed session open, the import's `ImportOutcome::Failed` and the copy's `CopyFailed` take
    them, while every log keeps `to_string()`, English; the session-open modal's
    `tx-session-open-failed` sentence is that error's `shown` alone, its `text` the bare English
    error. The run paths set it as `shown` too: a failed lazy `BEGIN`, the fresh-connection batch's
    per-statement failure and, inside the transaction note, the editor run's and Run All's session
    failures (under `core::tx`). So does every surface that only displays a `DbError` — the plan
    modal's `PlanState::Failed`, the export's read failure inside `export-failed` — and, through
    `export_writer_reason`, the writer's failure there too where it is only the read's: the writer
    is the nearer witness (a full disk fails it first) and its message is preferred, but a failed
    read reaches it as the reader's own `to_string()` on the `ExportChunk` channel, English, so
    where the two are equal the reader's error is shown as `db_error_shown` words it (no test can
    fail on this in English, the two being equal there) — the dump's table
    list, the blob panel, Server Activity's locks, sessions and kill, compare's database list and
    `compare-reading-failed`, the properties panel's table statistics, the account browser's
    principals and grants, the server variables viewer, the connection test's ping, the SQLite
    backup and the live monitor — and the schema tree's two load failures, a database's read as its
    `SchemaState::Failed` and the listing behind a tunnel as `LoadFailure::Server`'s `shown` (under
    `connection.rs`), each beside its English. **`db_error_shown` is for the eye alone**: nothing that is logged,
    persisted or sent to a model reads it. `english_shows_every_db_error_as_it_displays`
    (`app_tests`) holds it, and `modal_error_shown`'s `displayed()` and `text`, equal to
    `to_string()` over a `Connect`, a `Query`, a `QueryAt`, `Cancelled`, a `Refused` and two
    `Write`s. **The wording never replaces the English; it rides beside it.** The first cut did
    replace it, twice: `modal_error_shown` put the worded text in `ModalError::text`, which
    *Explain* and *AI fix* send, so a German interface sent the model German; and the copy's failure
    was one `db_error_shown` string for both the modal and `write_log::Entry::ended`, so it wrote
    German into the persisted write log. The copy now carries both, `(logged, shown)` in the app's
    `CopyFailed`, and a `ModalError`'s `text` is English by rule (under `core::model`).
    A failed DDL plan was the same lesson a third time, and the last `DbError` the window still
    showed as `to_string()`: one English string served the DDL modal and its write-log entry, and
    `schemaic-db` had already flattened it (`DdlError`'s `message: String`) before anything could
    word it. `DdlError` now carries a `DdlCause` (under `schemaic-db`'s `lib.rs`), and the app's
    `ddl_error_shown` words it through `i18n::ddl_failed` — `ddl-statement-failed`, inside
    `ddl-already-applied` when earlier statements are in effect — around `ddl_cause_shown`, which
    hands a `Db` to `db_error_shown`, passes a `Server` through as it came, and words the rest from
    `ddl-*` (`ddl_cancelled`, `ddl_dangling_foreign_key`, `ddl_breaks_view`, `ddl_no_databases`,
    and `ddl_rollback_failed` around its inner cause, worded by the same function). The DDL run's
    report carries `(logged, shown)` as the copy's does: the write log takes `to_string()`,
    `DdlOutcome::Failed` the wording; and the copy's settle failure, a `run_ddl` of its own, passes
    `ddl_error_shown` as its `shown` where it passed `to_string()` twice.
    `english_shows_every_ddl_failure_as_it_displays` (`app_tests`) holds the English equal to
    `Display` for every cause, bare and inside a `RollbackFailed`, with none, one and several
    statements already applied. What stays
    English: the server's or driver's
    words inside `Server`, the CSV parser's or a copy's read's inside `RowError::Read`, and the
    four `Query` sentences `schemaic-db` left plain on purpose — `COUNT(*) returned no row`, *the
    count came back as … not a number*, *A copy needs the multi-threaded runtime* and MySQL's
    `Unknown database '{db}'` — where only the prefix is worded. Its own export, settings, import
    and copy sentences, SQL Server's batch-continues note and SQLite's lock advice are no longer
    among it: they are a `QueryFailure`, worded as above. Its SSH, TLS,
    timeout, init SQL and session set-up sentences are no longer among it, nor cloud IAM's and
    Microsoft Entra's sign-in messages: they are a `ConnectFailure`, worded as above. A database-layer refusal inside `Refused` (SQL Server's
    blank-string and code-page refusals) is no longer among it: it is a `DbRefusal`, worded as
    above. The inner error of a SQLite import's `RollbackFailedToo` still is English carried as
    text, but no longer with its prefix: `sqlite::import_failure` carries the cause's own words
    (a write's `WriteCause::message`, without the `Complete` note attached before the rollback
    ran), and a `ROLLBACK` that fails on a connection SQLite has already rolled back (it reads as
    autocommit) leaves the error as it was. The squiggle's
    diagnostic (`intel::db_error_diagnostic`) still takes `to_string()`, prefix and all, as the
    `text` the AI fix prompt reads, its hover showing `db_error_shown` (`Diagnostic::shown_as`,
    above). The run paths' own `App`
    notes — not sent, timed out, a Stop's disclosure — used to be the other way round, the
    interface's wording in `text`, which *Explain* sent and the write log recorded; they now carry
    core's English as `text` and the wording as `shown` (under `core::tx`).
    `english_says_every_write_failure_as_core_does` holds it equal to `message` over every cause,
    every 1-row miss with and without a schema, each batch count both ways, every `IssueKind`
    with and without text, a `Read` and each `CopyStop`, and each with every rollback outcome and
    none.
    A Manual tab's run disclosures are the case where the choice is the hard part and the words
    the easy one: `tx::TxNote` is what a failed or cancelled run cost its transaction, chosen by
    `tx::run_failure_note`/`run_cancelled_note` (under `core::tx`, with every reason), and
    `i18n::tx_note_text` words each variant from `tx-*`. The counted three are each one message
    with a `$stmts` selector of `[0]`, `[one]` and `*[other]`, so every count is a whole sentence —
    core's English turns *was*/*were* and *nothing had run* on the same number — and
    `LostRolledBack { mysql: true }` is a message of its own, `tx-lost-rolled-back-mysql`, rather
    than the caveat appended as a fragment. `tx-cancelled` is `tx::CANCELLED`, and the not-sent
    pair `tx-not-sent-timeout`/`-cancelled` need no type, `not_sent_message` taking only a `bool`.
    `english_says_every_transaction_note_as_core_does` holds the interface's three functions equal
    to core's over all thirteen outcomes, seven states (counts 0, 1, 2 and 1234), the three engines
    and four statements that pick different notes.
    A message core *chooses* by engine capability takes the same shape with the choice as the
    value: the Export modal's replay-toggle hints were whole `&'static str`s picked per dialect, and
    are now `dump::DropHint` (`drop_hint`) and `FkHint` (`fk_guard_says_drops`), each with a
    `text()` that is the old English byte for byte — `drop_before_create_hint` and `fk_guard_hint`
    remain as its wrappers, for core's tests. The capability decision stays in core; `dump_view`
    words only the answer. Where core's answer is an `Option` with one sentence per case, it needs
    no new type: the caller asks it and maps the `Some` to its own words, `.map(|_| …)`, as
    the top bar does `Environment::badge_label` and the connection form its field notes
    (`SslMode::caveat`, `AuthMode::tls_note`, `read_only_caveat`, `named_instance_port_note`,
    under `connection_form.rs`) — core decides whether, the interface words it.
    **A sentence with optional parts is one message with a selector per part, not fragments
    assembled in code** — the lock-wait card's is the pattern. `activity-lock-wait-sentence` takes
    `$waited`/`$held` as an age or as `none` where the server withheld one (no age reads as that),
    `$doing` as the holder's state (`idle-in-tx`, `blocked`, `running`, `idle`) and `$others` with
    an exact `[0]` variant ahead of `[one]`/`[other]`. A translation has to be able to move the
    parts, not only reword them: the German makes the holder a relative clause with its verb last
    (`die Sitzung { $holder } … hält`), which no English-ordered run of fragments could have let it
    do.
    **A typed message's renderer lives beside the code that shows it** — `alias_rename.rs`,
    `install_cli.rs`, `settings.rs`, `grid.rs`, the app's `main.rs` — and `ui/i18n.rs` holds only the ones another file or crate
    calls: `param_error` for the app, `guard_reason_text` for the app's run guard and `lib.rs`'s
    `ScriptRequest::approved`, `statement_timeout`, which `settings.rs`,
    `connection_form.rs` and the app all call, the three export notes, which `grid.rs`,
    `result_diff_view.rs` and `dump_view.rs` share, `skip_reason_shown`, which the import modal
    and the app's paste field share, `read_refusal_text` and `import_error_text` for the app's
    import probe, check and load, `write_failure_text` for the app's Commit, import and copy
    failures, `db_query_failed`, `db_connection_failed`, `db_query_cancelled`,
    `connect_failure_text`, `query_failure_text` and `db_refusal_text` for the app's `db_error_shown`, the crate holding no `DbError`, the six
    `ddl_*` for its `ddl_error_shown`, for
    the same reason, and the grid's three filter notices — `staged_requery_note`
    for `grid.rs`, `staged_landing_note` and `reread_without_filter_note` for the app — worded
    from `filter`'s constants, `requery` refusing with the value `filter::HoldsStagedWork` — and,
    public for the app's run paths, `run_failed_message`, `run_cancelled_message` and
    `not_sent_message`, with core's signatures, and for the app's kill and compare actions,
    `activity_read_only_refusal` and `compare_incomparable`, plain arguments as core's take, and
    `tunnel_refusal` for its connection lookups, and the four agent renderers above for `ai.rs`,
    `cursor.rs` and `main.rs` — and three more the list missed until the review after v0.35.0:
    `notice_text`, which `lib.rs`'s startup recovery report and the app's keyring save notice
    share; `elsewhere_note_text`, for the app's DDL apply, whose note `lib.rs`'s `DdlOutcome`
    carries; and `issue_kind_message`, which `import_view.rs` calls and `i18n.rs`'s own
    `row_error_text` builds on.
    Core's own stays English for the CLI, the
    MCP server and the AI prompts, and so does every use of it that is a key or a prompt: the
    tree's persisted `objgrp:`/`obj:` expansion keys, the *AI explain* prompts.
  - **Isolation marks are off**, set after every catalogue load, because Fluent's default
    U+2068/U+2069 around each placeable made an English message with an argument differ from the
    literal it replaced (`a_message_with_an_argument_carries_no_isolation_marks`). Converting a
    label must leave English output byte-identical: every phase of the conversion promised it, because
    the suite's assertions on English wording are the regression net for the whole conversion.
  - **The language is read once, at launch**, before any view, and never changes while the app
    runs; nothing takes a reactive dependency on it, which spares every label one for a setting
    changed once. The setting says so where it is changed.
  - **`SCHEMAIC_LOCALE` overrides Settings and the OS**: `SCHEMAIC_LOCALE=de` (or `sq`, `it`, `nl`,
    `fr`, `es`, `ro`, `bg`) draws that catalogue whatever the setting says, and would reach one not
    yet shipped — none is today — which is also how a box is checked
    against longer text, and a string nobody moved into the catalogue shows, reading English among
    the German. The setting and the OS reach only `locale::SHIPPED`, which a language joins when
    its catalogue is complete and the user decides to ship it; all nine are on it. The two
    pseudo-locales that once did the layout job (`pseudo`, `pseudo-en`) are removed, and the
    variable now reads either as a key it does not know.
  - **Layout for longer text** — phase 6 of *Interface translation*, landed. A summary that points
    at the passages holding the detail, not a restatement of them. The phase was worked under a
    doubled pseudo-locale, every label said twice, since removed; **layout is now checked under
    the real, longer catalogues** — German running longest — with `SCHEMAIC_LOCALE=de`. **The
    rule: a box beside text either measures its text or ellipsizes it — nothing fixed-width just
    clips.** The pieces, each recorded where it
    lives:
    `widgets::ellipsized_text` (under `widgets.rs`) — `min_width(0)` on the text *and* on the
    tooltip wrapper (*`.tooltip()`'s wrapper is the flex item* under *Floem 0.2 gotchas*), the
    tooltip only when the line is actually cut — behind `footer_error`'s modal statuses,
    `menu_row_face`'s action rows under Manage connections' and Users' lists, the Properties
    *Count rows* hint, the palette row's secondary, the grid row panel's name and type, and Import's
    mapping header names; `flex_shrink(1000)` on the half that gives way first (the row panel's
    type, the palette's hint); `form_hint_style`'s `width_full`, so a hint wraps; `section_title`
    and `panel_group_header` ellipsizing so a header's buttons and count never go; the fact panels'
    label column measured (`widgets::fact_label_w`); the Ctrl+Enter run menu's width measured
    (`consts::run_menu_w`, under *Floem 0.2 gotchas*); the ER diagram's count pills dropping one at
    a time (`core::erd::fit_toolbar`); Settings' page at 500 with *Settings file*'s buttons stacked
    (`settings.rs`); Connection import's source buttons wrapping; and the calendar's day cell at 33
    (`day_w`, for longer month names).
  - **No right-to-left interface, and no Chinese, Japanese or Korean one — standing decisions,
    not pending work.** A right-to-left interface means mirroring, and nothing underneath can:
    Floem 0.2 has no layout direction, no start/end insets and no `text-align`, taffy 0.4 does
    not mirror, and the UI hard-codes left and right in a few hundred style calls, so it would be
    a project of its own. A CJK interface cannot be romanised — pinyin without characters is
    ambiguous, and a romaji or romanised-Korean interface is essentially unseen — so it is a
    proper translation or none, and a proper one would wait on the fonts below. Neither is a goal.
    **Data in those scripts is another matter**, and is shown whatever the interface's language.
    Right-to-left cell data shapes and reorders through cosmic-text but always sits left-aligned;
    aligning a cell by its first strong character would be cheap, and is not planned. Only IBM
    Plex is bundled (`fonts.rs` — Latin, Cyrillic, Greek), so CJK, Arabic and Hebrew on screen
    rely on cosmic-text falling back to the system's fonts, which is unverified on macOS and
    Linux; the diagram PNG has a fallback of its own (`export_options`, under `erd_raster.rs`).
    With the isolation marks off (above), a right-to-left value placed in a message is not
    protected from reordering the sentence round it either.
  - **Three tests guard what the compiler cannot**: `every_catalogue_parses_cleanly`, because a
    malformed translation is only logged at run time and falls back to English unseen;
    `no_translation_outlives_its_english_message`, for a renamed or deleted English id; and
    `a_translation_reads_only_the_variables_english_is_given`. A message a translation lacks is
    not caught, by design: it reads in English (`a_message_german_lacks_reads_in_english`).
- **Reactive text**: use `dyn_container` (no `floem::views::label`).
- **There is no accessibility tree, and there is nothing in this repository that can add one.**
  Floem 0.2.0 ships no AccessKit integration and no a11y surface of any kind — grepping the crate
  for `accesskit`/`accessibility`/`a11y` turns up only the English word in two doc comments about
  platform config — so nothing the app builds is exposed to Narrator, VoiceOver or Orca, whatever
  it is labelled. What the app *does* have is keyboard operability and a legible size, and those are
  the axes worth spending on: `FocusRing`, `focus_root_with_ring`, spaced tab indices,
  `widgets::accept_launch`, the `shortcuts.rs` table with the test that fails when a binding has no
  row — and the **interface scale** (`UiScale`), which is why the chrome's type is a set of
  functions rather than a 13px constant. The README says this out loud under *Accessibility* rather
  than leaving someone to discover it after the download; revisit if a later Floem grows the layer.
