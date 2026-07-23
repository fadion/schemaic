# Schemaic

A native SQL editor (Rust + [Floem](https://github.com/lapce/floem) 0.2.0), MySQL/MariaDB-first,
Zed-inspired, aiming to replace DataGrip.

## Crates

- `schemaic-core` — models, persisted UI state (`persist.rs`), transcript types, and the pure,
  unit-tested SQL/edit/export logic (UI/app keep thin wrappers; regression tests live here):
  - `sql.rs` — one `skip_noncode` tokenizer → statement splitting, unsafe-statement guard, AI
    read-only gate. The *single* SQL boundary lexer; `tokenize_range`/`syntax_errors`/`sql_highlight`
    all build on it so string/`#`/`--`/`/* */`/backtick boundaries agree by construction.
  - `edit.rs` — `analyze_edit` → `EditModel` (write-back updatability analysis).
  - `export.rs` — CSV/JSON/SQL export (incl. CSV formula-injection guard).
  - `diff.rs` — `line_diff`/`build_diff_rows` (Ctrl+K preview).
  - `history.rs` — query-history model (`push`/`clear_conn`/`preview`/`relative_time`),
    persisted to `history.json`.
  - `format.rs` — per-column display formatters (`ColumnFormat`/`apply`: epoch→datetime, bytes,
    bool). Display-only; edit/copy stay raw. Persisted to `format.json`.
  - `schema::TableInfo::create_ddl` — `CREATE TABLE`/`VIEW` skeleton.
  - `plan.rs` — `QueryPlan::from_result` parses an `EXPLAIN` result into a table + heuristic
    warnings (full scan / filesort / temp table); `to_prompt_text` for the AI.
  - `text_ops.rs` — Ctrl+/ `toggle_line_comment` + `find_matches`/`replace_all`/
    `contains_ignore_ascii_case` (find bars). Pure, ASCII-case-insensitive, byte-offset-preserving.
  - `sqlfmt.rs` — `format_sql` (Ctrl+Alt+L pretty-printer): re-flows whitespace/indent/line-breaks
    "block" style, **preserving keyword case**; built on `skip_noncode` so comments/strings/backtick
    idents pass through untouched; indent follows editor tab-width/soft-tabs.
- `schemaic-db` — MySQL/MariaDB (`mysql_async`) + SSH tunnels. Populates each result column's
  `origin` (real table/column + key flags) from the wire protocol. Connection **identity** is the
  `Db` handle (`Db::connect(&Connection, tunnel_port)`), not a `mysql://…` URL — credentials go
  through `OptsBuilder` (passwords with `@ / # ? %` need no escaping; no plaintext URL anywhere).
  `fetch_query`/`run_batch`/`fetch_schema`/`ping`/`commit_writes`/`refetch_rows` are `Db` methods
  taking the target DB per call. SSH tunnels return a `TunnelHandle` (drop → port freed) with
  keepalives + TOFU host-key verification (`ssh_known_hosts.json`).
- `schemaic-ai` — persistent `claude` CLI session (stream-json), turn parsing.
- `schemaic-term` — terminal panel + shell.
- `schemaic-ui` — the Floem UI. The central `Ui` struct (threaded everywhere) is split per-domain:
  `Copy` signal bundles (`TabsUi`/`SchemaUi`/`ConnUi`/`AiUi`/`TermUi`/`LayoutUi`/`OverlayUi`) +
  `Rc<…Actions>` callback bundles — so `ui.run` is `ui.tab_actions.run`, `ui.db_nodes` is
  `ui.schema.db_nodes`, the tabs signal is `ui.tabs_ui.tabs`. Modules:
  - `consts.rs` — layout/dimension constants (glob-imported).
  - `widgets.rs` — reusable widgets: `menu_panel`/`MenuEntry`, `modal_title`/`panel_style`/
    `menu_item_style`, `window_size`, `autohide`/`shift_hscroll`/`wheel_hscroll` scroll wrappers,
    `section_title`/`centered_msg`/`toggle_icon`, `measure_text_px`, `jump_to_bottom_button`.
  - `markdown.rs` — AI-chat `render_markdown`/`CodeActions`/`code_block` (pulldown-cmark).
  - `settings.rs` — the three settings modals + shared controls.
  - `connection_form.rs` — Manage Connections modal + password-mask (+ tests).
  - `diff_view.rs` — Ctrl+K diff preview. `history_panel.rs` — Query History right-column panel.
  - `plan_view.rs` — Query Plan modal (`EXPLAIN`/`EXPLAIN ANALYZE` table + warnings + "Ask AI"),
    via `TabsActions::run_plan` → `Db::explain`.
  - `ai_panel.rs` — AI Assistant panel (`ai_panel`/`message_bubble`/`render_segments`/`tool_chip`/
    `assistant_footer`).
  - `overlays.rs` — absolutely-positioned popups: connection/active-db/schema menus, schema context
    menu, generic grid popup, Find-Anywhere, error modal.
  - `schema_tree.rs` — SCHEMA sidebar (`schema_panel` + db/table/column/key row builders + keyboard
    nav). `completion.rs` — SQL autocomplete (`recompute_completions`/`accept_completion`/
    `completion_popup` + context engine; `SQL_KEYWORDS` pub(crate)).
  - `tabs.rs` — query-tab strip. `grid.rs` — the whole results grid (`GridState`/`GridCtx`;
    `results_view`/`loaded_view` are the entry points). `editor_pane.rs` — SQL editor pane
    (`query_pane` + Ctrl+K popup, typo squiggles, statement highlight, custom scrollbars).
  - `theme.rs`/`themes.rs`/`icons.rs`/`fonts.rs`/`sql_highlight.rs`.
  - `lib.rs` (~3.2k lines, being split further) — the `Ui` struct + bundles, shared model/state
    types, `workspace`/`body`/`center`/`header`/`footer`, resize handles, `edit_field`/`FieldCfg`,
    terminal panel.
- `schemaic-app` — `main.rs` wires signals + callbacks and builds the `Ui`; also the built-in MCP
  server (`--mcp-serve`) the AI panel talks to. A query tab's identity is `(conn_id, database)`;
  the app resolves `conn_id` → `Db` at run time (`db_for`), so a tab keeps its connection after a
  switch. The MCP subprocess gets its DB endpoint as JSON in `$SCHEMAIC_MCP_ENDPOINT` via a
  per-session temp `--mcp-config` file (removed on drop) — never argv, so credentials don't leak
  to other same-user processes. Pure clusters split out: `claude_cli.rs` (`claude` binary
  discovery — PATH/PATHEXT/override) and `ai.rs` (`AiSession`/`start_ai_session` streaming,
  MCP-config plumbing, `ai_context`/`inline_system_prompt`). Reactive wiring (`app_view` closures)
  stays in `main.rs`.

## Architecture invariants (don't regress these)

Re-introducing the anti-patterns these guard against is a regression:

- **One SQL boundary lexer.** Any code scanning SQL for string / `-- ` / `#` / `/* */` / backtick
  boundaries MUST build on `schemaic_core::sql::skip_noncode` (statement split, WHERE guard, AI
  read-only gate, `tokenize_range`, `syntax_errors`, `sql_highlight`). Never hand-roll a second
  scanner — five drifting copies was the original bug.
- **Connection identity is the `Db` handle / `conn_id`, never a `mysql://user:pass@host/db` URL.**
  Credentials go through `OptsBuilder`; never in a URL, argv, or log. The MCP subprocess gets its
  endpoint via a temp `--mcp-config` file, not argv. Don't add new plaintext-secret surfaces.
- **Own per-entity signals in a child `Scope`; dispose it *deferred*.** A `Tab`/`ConnNode` creates
  its signals in `parent.create_child()`; removal disposes that scope via
  `exec_after(Duration::ZERO, …)` — one tick later, after the keyed `dyn_container` has unmounted
  the old view. Synchronous disposal frees signals a still-mounted view reads this frame → panic.
  Same for any "replace + free" of scoped state.
- **Themable colors reach reactive styles as `fn() -> Color`, never a captured `Color`.** A `Color`
  read once at build freezes and won't follow a live theme switch; pass the fn and call it inside
  the `.style(move |s| …)` closure (see `FieldCfg::background`).
- **Pure logic lives in `schemaic-core` with unit tests** — SQL boundaries, edit-model analysis,
  export (incl. CSV formula-injection guard), diff, DDL. The UI keeps thin wrappers.
- **Write-back is transactional with a 1-row safety net.** `commit_writes` runs a `GridWrite`
  (DELETEs → UPDATEs → INSERTs) in one transaction, each statement required to affect exactly 1 row
  (else roll back all) — so an over-optimistic updatability analysis can't corrupt data. Commits
  with inserts/deletes full-re-run the query (membership/order changed); pure-UPDATE commits splice
  in place.
- **Identifier scanning treats bytes `>= 0x80` as word bytes** so Unicode identifiers tokenize whole
  (`is_word_byte`, `tokenize_range`, `syntax_errors`).
- **Splitting `lib.rs` / `main.rs`:** grep the line range for interleaved unrelated `fn`s first; a
  helper still used by code that stays goes to `widgets.rs` (glob-imported), not the new leaf
  module; mark cross-called items `pub(crate)`; build + `cargo fmt` + smoke-launch each step.

## Build & run

- `cargo build` / `cargo run -p schemaic-app`.
- **Windows:** if the app is running, the linker can't overwrite `target/debug/schemaic.exe`
  ("Access is denied"). Stop it first (`Get-Process schemaic | Stop-Process -Force`).

## Commits & releases

- **Never commit unless the user explicitly asks.** Making edits does not imply committing — leave
  changes in the working tree. Same for `git tag`/`git push`. Amending is fine when the user is
  iterating on a commit.
- **Conventional Commits** — `type(scope): subject`, imperative, no trailing period, lower-case
  after the colon. Types: `feat`/`fix`/`refactor`/`perf`/`docs`/`test`/`chore`/`build`/`ci`. Scope
  = the crate/module the change centers on (`grid`, `editor`, `schema`, `ai`, `sql`, `theme`, `db`,
  `ci`…); omit only when cross-cutting. Optional body (blank line first) explains the *why*. Every
  message ends with the trailer `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>`. Example:
  `feat(grid): add row cloning via context menu`.
- **Version bumps are explicit-only.** Bump only when asked; never as a side effect of an unrelated
  commit. Edit **one** place — `[workspace.package].version` in the root `Cargo.toml` (all crates
  inherit via `version.workspace = true`; never a per-crate `version`). Commit as
  `chore: release vX.Y.Z`.
- **Releases are tag-driven.** Bump → commit → `git tag vX.Y.Z && git push origin vX.Y.Z` (keep tag
  and `Cargo.toml` in sync). The tag triggers `release.yml` (Linux + Windows binaries → GitHub
  Release); `ci.yml` runs fmt + clippy (`-D warnings`) + `cargo deny` + build/test on push/PR. Keep
  the tree green before tagging.

## UI conventions

- **No pointer cursor on buttons/icons** — native apps keep the arrow cursor; a pointer feels
  web-like. Use the default; reserve `CursorStyle::Text` for text inputs (a genuine hyperlink may
  keep `Pointer`).
- **Colors live in `theme.rs`** as named fns — add one rather than inlining a hex literal. They read
  the *active* theme from `themes.rs` (reactive), so calling one inside a `.style(…)` closure follows
  a live theme switch for free.
- **Theming (`themes.rs`)**: two independent axes — `UiTheme` (chrome: dark/light) and `EditorTheme`
  (editor surface + syntax tokens: One Dark Pro / Tokyo Night / Catppuccin Latte). A theme is a flat
  struct of named colour roles (hex). Active themes live in `Scope`-owned global `RwSignal`s;
  `theme::set_ui`/`set_editor` swap them. The choice is persisted (`ui_theme`/`editor_theme` in
  `UiState`) and seeded via `theme::init` before the view builds. Editor tokens re-highlight on
  switch because `SqlStyling::id()` returns `theme::editor_generation()`.
  - **Live-switch caveat**: a colour read *inside* a reactive `.style` closure updates instantly; one
    captured *by value* freezes at build time. Prefer `fn() -> Color` for anything themable (see
    `FieldCfg::background`).
- **Reactive text**: use `dyn_container` (no `floem::views::label`).
- Small visual tweaks: build only, let the user verify. Screenshot harness for new features /
  behavior debugging, or when asked.

## Floem 0.2 gotchas (learned the hard way)

- **One `on_scroll` per scroll** — setting it twice clobbers. `autohide` sets its own; a scroll
  needing custom `on_scroll` must inline `autohide_state()` (results grid + AI convo).
- **No `opacity` property.** Fade via color alpha (`multiply_alpha`) + `.transition_*`. Toggle
  visibility with `.hide()`/`.flex()` (display none/flex).
- **Inherited color doesn't animate to a child.** A parent's `.transition_color()` won't fade a
  child svg's `currentColor` — set color + transition on the element itself.
- **Style precedence**: a view's own (direct) style beats ancestor class styles; nearest ancestor
  class wins. Nest class overrides accordingly (dropdown popup restyle nests under `ListClass`).
- **`DoubleClick` consumes the second `PointerUp`** — clear drag/press state in the double-click
  handler too, not only in `PointerUp`.
- **Absolute overlays** (placeholders, action bars) intercept clicks — add `.pointer_events(|| false)`
  so clicks fall through.
- **Deferred layout**: `exec_after(Duration::ZERO, …)` runs after layout settles — so
  `scroll_to(bottom)` clamps against new content height, not stale.
- **`RwSignal::set` never dedups** — setting a signal to its current value still notifies, re-running
  dependent `dyn_container`s (which dispose + rebuild their child scope + owned signals). Guard panel
  reveals: `if !matches!(right_panel.get_untracked(), Ai) { set(Ai) }` — a redundant `set(Ai)` while
  the AI panel is open disposes its `elapsed_ms` mid-update and the rebuilt footer panics on the
  freed signal.
- **Don't read a locally-scoped signal inside a `dyn_container` child keyed on a *parent/shared*
  signal.** The child rebuilds when the shared signal changes — and if it changes *while the
  enclosing view is disposing* (e.g. `active_db` updates as the query pane is replaced on opening a
  table), the rebuild reads the freed local signal → panic. Fix: read the local signal in a stable
  outer scope and let the child inherit (put the hover `color()` on the parent `h_stack`, not the
  `active_db`-keyed child). Reading *global* signals (theme, `connections`/`active_conn`) there is
  fine — they never dispose.
- **No `text-align` in Floem 0.2.** `text_input` paints at a fixed left origin, clips-to-cursor on
  overflow; `Style` only has `text_overflow`. To right-align an inline editor (numeric grid cells),
  pad left by `col_w − measured_text_w` — measure with a throwaway `TextLayout` at `FONT_BODY` (same
  global `FontSystem` → pixel-exact), recomputed reactively on the buffer (`grid::measure_text_px`).
- **SQL editor padding is a no-op; inset via a wrapper.** The editor is a scroll view — its own
  `padding_*` is ignored. Wrap it in a container carrying the border + padding (`editor_box`); the
  editor fills it flush. Top padding shifts the content origin, so `points_of_offset`-anchored
  overlays (completion popup, statement highlight, squiggles, Ctrl+K, run menu) each add back
  `EDITOR_PAD_TOP` to their `y`. The built-in scrollbars float at the *content* edge (can only inset
  inward), so they're **replaced with custom overlay scrollbars** (`v_scrollbar`/`h_scrollbar` in
  `editor_area`): built-in bars hidden (zero-`Thickness` + transparent `Handle`), two `empty()`
  thumbs pinned to the border (`inset_right/bottom(3)`) with `autohide_state()`. Geometry from
  `ed.viewport` (offset `x0`/`y0` + visible `width()`/`height()`) vs. content (`ed.max_line_width()`,
  `(ed.last_line()+1) * ed.line_height(0)`; `ScrollBeyondLastLine` = false), in `v_geo`/`h_geo`
  shared by the style closure and drag handler. Thumb `.style()` reads `viewport.get()` **and**
  `query.get()` (content size isn't a signal). **Draggable**: `PointerDown` records grab offset +
  `id.request_active()` (pointer capture); each `PointerMove` sets `ed.scroll_to.set(Some(Vec2))`
  (it's `Option<Vec2>`, not `Point`). Thumbs use `scrollbar_hover()` + `CursorStyle::Default`.
- **Shift+wheel → horizontal scroll in the editor.** The editor owns its scroll internally, so
  `shift_hscroll` can't reach it. Register a `PointerWheel` listener on the internal scroll view —
  reached via `ed.editor_view_id.get_untracked().and_then(|c| c.parent())` (the content view's parent
  *is* the scroll) + `ViewId::add_event_listener`. Floem's `Scroll` runs registered listeners in
  `event_after_children` before its default scrolling, so returning `Stop` for shift+wheel suppresses
  vertical scroll; push the horizontal delta through `ed.scroll_delta` (Windows delivers shift+wheel
  as vertical `delta.y` → map to x). Event flow: a child's `event_after_children` runs *between* the
  parent's before/after, and a child that consumes a pointer event stops propagation — so a
  `PointerWheel` `on_event` on an ancestor never sees a wheel the inner scroll consumed. Target the
  scroll's own `ViewId`.
- **A programmatic `cursor` change shows a phantom caret on an *unfocused* editor.** `edit_field`'s
  signal→doc reconcile sets `cursor` to preserve caret position, but floem's internal "reset cursor
  blinking" effect tracks `ed.cursor` → `cursor_info.reset()` → shows + blinks the caret even with no
  focus (made Name/User/Password look focused). Fix: only write `cursor` in the reconcile when
  `focused.get_untracked()`. The stale offset is safe — floem clamps `offset.min(text.len())` and a
  real click resets it. (Don't hide the caret *after* the write — floem's reset effect runs after
  yours and re-shows it.)
- **A perpetual self-rescheduling `exec_after` tick must read signals with `try_get_untracked`.** The
  terminal cursor-blink tick reschedules forever; at shutdown the scope disposes its signals and the
  last timer panics on `get_untracked`. Guard every read with `try_get_untracked` and stop
  rescheduling once any returns `None`.
- **`.clip()` makes a flex item shrink-to-content — it won't stretch to its parent**, so a
  `flex_grow` spacer inside it collapses (right-aligned children stop reaching the edge). Put
  `.clip()` on a container with a *definite* size (the fixed-width `v_stack`), not the flex row you
  depend on for stretch. (Bit the find/replace bar's `All` alignment.)
- **`s.hide()`/`s.flex()` (display none/flex) beat height/scale for a reactive show-hide** — adds/
  removes the element from layout cleanly (no clip/overflow/leftover space). Prefer it to animating
  height when you don't need the animation.
- **In-flow reveal animations are janky; only `.absolute()` transforms animate smoothly.** A
  `.transition(Height, …)` on an in-flow element reflows its container every frame and Floem only
  steps transitions on redraw ticks → ~5fps. Smooth animations here (Ctrl+K expand) animate an
  `.absolute()` overlay's inset/size so nothing reflows. Either animate an absolute overlay or toggle
  `display`.
- **`Style::rotate` is RADIANS** (kurbo `Affine::rotate`, centre-pivoted) — pass `FRAC_PI_2` for 90°,
  not `90.0` (~14 turns → glyph vanishes). `scale` is a `Pct`. Both transition via
  `.transition(Rotation/ScaleX, …)`. Even correct, a transform-transition on a small `svg` proved
  unreliable — an icon swap is the safe chevron-flip fallback.
- **`edit_field` lets you control Escape/blur; `text_input` eats Escape.** `text_input` handles
  Escape internally (`clear_focus`, `event_before_children`, returns `Stop`), so your
  `on_event(KeyDown)` never sees it. `edit_field` routes Escape to `on_escape` and focus-loss to
  `on_blur` (guarded to skip the mount run) — use it for discard-on-Escape / commit-on-blur (inline
  rename, find/replace).

## Popup menus (`menu_panel`)

Custom themed overlays, not Floem's native `Menu` (native renders OS-styled, clashes with the dark
theme). `menu_panel(entries: Vec<MenuEntry>, close)` takes `Action`/`Sub`/`Separator` entries and
renders the themed panel; the caller positions it absolutely. Used by the schema right-click menu
(`context_menu_overlay`).

- **Nested submenus**: a `Sub` entry hover-expands a child `menu_stack` anchored to the parent row's
  right edge (`inset_left_pct(100.0)` + `inset_top(-6.0)`). Recursive — each level owns its `open_sub`
  signal.
- **Hover intent (no timers)**: entering a leaf clears `open_sub`, entering a submenu row sets it;
  nothing closes on leave. The submenu is flush with the panel's right edge, so a diagonal move never
  crosses a gap — the close-on-diagonal problem is avoided structurally.
- **Dismissal**: the panel `on_event_stop`s its own pointer-downs, so the root "pointer-down anywhere
  closes" handler (in `workspace`) fires only for outside clicks. Escape and any action also call
  `close`. Submenus are view-tree descendants, so their clicks are absorbed by the root panel too.
- **Edge-flipping**: submenus flip left (`inset_right_pct(100)`) past the right edge and shift up past
  the bottom — from the parent row's window position (`on_move`/`on_resize`) + the live `window_size()`
  global (set from `workspace`'s root `on_resize`). `popup_menu_overlay` flips the whole panel the
  same way at the cursor. Size checks use conservative estimates (width ≈ 210, row ≈ 34) so there's no
  open-then-flip flicker.
- **Two menu channels**: the schema tree uses `ui.context_menu` (typed `CtxMenu`) +
  `context_menu_overlay`; everything else uses the generic `ui.popup_menu`
  (`RwSignal<Option<Vec<MenuEntry>>>`) + `popup_menu_overlay`. Both overlays live at the workspace
  root (window coords) and close on the root pointer-down.

## Data grid (results grid)

`grid_view` (in `grid.rs`) is built around `GridState` — a `Copy` bundle of `RwSignal`s created once
per result set and threaded into every cell/handler. It holds column widths, the selection
(`active`/`anchor` in **display** coords so selection stays put visually on sort), the display→data-row
`order`, the value-viewer/freeze/edit toggles, the `dirty` edit map, and `vp`/`scroll_to`/`focus_id`
for keyboard nav.

- **Two panes** side by side (`h_stack`): a **frozen pane** (row-number gutter + optional frozen
  column) and a horizontally-scrolling **data pane**. Rebuilt by a `dyn_container` keyed on
  `(sort, frozen)`. **Freeze is per-column, any column**: `gs.frozen` holds the frozen column's
  *absolute* index, set from the header right-click menu (no toolbar button). The data pane renders
  `data_cols` = `(0..ncols)` minus the frozen index (an `Arc<Vec<usize>>`); cells keep their
  *absolute* `ci` so selection/resize/sort stay consistent. Frozen pane width = `GUTTER_W + widths[frozen]`.
- **⚠️ Scroll-sync rule (cost a hang):** a scroll view must **never both read and write the same
  offset signal** — it re-enters its own layout and hangs the UI thread. Strict one-writer/one-reader:
  the **data pane writes `vscroll`** (`on_scroll`) and reads `gs.scroll_to` (keyboard channel); the
  **frozen pane reads `vscroll`** (its `scroll_to`), has **no `on_scroll`**, and blocks its own wheel
  (`on_event(PointerWheel, |_| Stop)`).
- **Column widths** (`gs.widths`) are estimated from content on load; the header's `col_resize_handle`
  drags to resize (moving-view trick) and double-clicks to auto-fit. Cells read `gs.widths` in
  `.style()` so resize is live. Every cell/header uses `flex_shrink(0)` so the row overflows (enabling
  h-scroll) instead of squeezing.
- **Selection**: click sets `active`+`anchor`; `PointerEnter` while `selecting` extends the range
  (drag-select, no capture); gutter click selects the row. Copy (Ctrl+C / toolbar) emits TSV; a lone
  cell copies its raw value.
- **Right-click menus** (generic `menu_panel` / `ui.popup_menu`): a header offers `Copy › CSV / JSON`
  of that column's values (`export_column_csv`/`_json`); a data cell offers `View`, `Edit` (editable
  cells only), `Copy`, `Set to NULL` (editable **and** nullable — stages `dirty` `None`), and
  `AI Summary` (reveals the AI panel, prompts with source table + column for context). The grid's app
  context (`source`, `db_nodes`, `connections`/`active_conn`, `popup`, `summarize`, `dismiss`, …) is
  bundled in `GridCtx`, threaded `results_section → results_view/multi → loaded_view → grid_view`,
  then stashed in `GridState` (whose `Rc` callbacks live in `RwSignal<Option<…>>` since it's `Copy`).
- **Menu dismissal**: grid cells consume the pointer-down (drag-select), so the root handler never
  fires inside the grid; cell/header/gutter click handlers call `gs.dismiss` (closes both
  `ui.popup_menu` and `ui.context_menu`, guarded).
- **Value viewer**: no toolbar toggle — opened from the cell `View` item (or double-click / Enter),
  closed by ✕ or Esc. A read-only, word-wrapped, auto-growing `edit_field` (`multiline` + `read_only`)
  that follows the active cell via an effect → `text_sig`. Grows to a cap then scrolls; the cap is a
  reactive `FieldCfg.max_rows` from the results-panel height (`(panel − 172) / 19` rows) while the
  grid keeps `min_height(120)`. `edit_field` has `max_rows` + a `viewport`-tracking effect that
  recomputes wrapped line count on layout (`.update` only fires on edits, so programmatic multiline
  text would measure at zero width → one line). Wrapping needs a bounded width (`.width_full()`).
- **Inline edit writes back to the DB.** Per-column *provenance*: `schemaic-db` runs on
  **`mysql_async`** (sqlx's MySQL driver discards `org_table`/`org_name`/key flags), so each `Column`
  carries `origin: Option<ColumnOrigin>` — real `database`/`table`/`column` + `ColumnFlags`
  (pk/unique/not_null/auto_increment), or `None` for an expression (read-only). `analyze_edit` builds
  an `EditModel`: which columns are editable + each base table's WHERE key (schema PK first —
  authoritative for composite keys; else a fully-present unique NOT NULL index; else wire PK flags
  when schema isn't loaded). Flow: double-click an editable cell (or Enter) → inline editor; **Enter**
  stages into `gs.dirty` and paints the cell `grid_edit_staged()`; **Ctrl+Enter** / toolbar ✓ calls
  `commit_grid` → a `GridWrite { updates, inserts }` → the app's `commit_edits`. On success the app
  re-runs the query; on failure the error shows in the toolbar and green edits stay. No global "Edit"
  toggle. A read-only cell's double-click opens the value viewer.
- **Row actions: new / clone / delete.** Gated on a single writable table (`EditModel::insert_target()`;
  hidden for joins / read-only), committed in the shared `GridWrite` transaction (`commit_writes` runs
  **deletes → updates → inserts**, each exactly 1 row).
    - **New (INSERT):** toolbar **"+ Row"** appends a blank pending row (`gs.new_rows`), rendered below
      real rows with a `*` gutter marker + faint green wash, first editable cell opened. Cells stage
      via `stage_new` (unset = server default; `Some("")` clears to default). Unset cells preview
      `<auto>`/`<required>`/`<null>`/`<default>` (from wire `auto_increment`/`no_default`/`not_null`).
      Tab/Enter hop cells (`advance_edit`).
    - **Clone:** right-click **Duplicate row** seeds a pending row via `add_cloned_row` (every editable
      column except auto-increment).
    - **Delete:** right-click **Delete row** or the **Del** key marks a real row (`gs.del_rows`) with a
      red wash; marking clears its staged edits. `build_deletes` keys each `RowDelete` by the table's
      `key_cols` + original values.
  Inserts/deletes change membership/order → those commits **full-re-run** the query (pure-UPDATE
  splices in place). A NOT-NULL-no-default omission or duplicate-key clone fails the transaction and
  surfaces the error — nothing half-applied.
- **Type-aware headers** show `type_name` under the name (two-line, `GRID_HEADER_H`). A sorted column's
  name + chevron use `grid_sort()`; a column with selected cells gets a `grid_col_sel()` header
  background. **Key icons** (PK = gold key-round, single-col index = blue key-square, FK = purple
  key-square; colours shared with the schema tree via `key_primary/index/foreign`) come from
  `column_key_map`, cross-referencing the tab's `source` against the loaded schema (`db_nodes`). Only
  populated when the tab was opened from a table with schema loaded; arbitrary SELECTs get none.
  Nullable markers deferred.
- **Column virtualization.** Both rows (`virtual_stack`) *and* columns are virtualized: the header and
  every data row render only the columns intersecting the horizontal viewport (+ a small overscan)
  between two width-preserving spacers, so a wide table builds ~10-14 cells/row instead of all of them
  (a 100k×50 inertial fling stays smooth). The visible window is a `ColWindow` (`start..end` into
  `data_cols` + left/right spacer px) from a `create_memo` — it recomputes on scroll but, since memos
  dedup on `PartialEq`, only *notifies* (rebuilding header + row cells) when the visible column set
  changes, not every pixel. Header and every row read the **same** `win` memo, so the panes stay
  aligned. Invariant: `gs.widths` stays full-length and each row's total width = `sum(widths[data_cols])`
  (spacers make up the hidden columns), so `h_off`/`scroll_to` geometry is unchanged —
  `scroll_active_into_view` sums in data-pane space (excluding the frozen column) to match.
