# Schemaic

A native SQL editor (Rust + [Floem](https://github.com/lapce/floem) 0.2.0), MySQL/MariaDB-first,
Zed-inspired, aiming to replace DataGrip.

## Crates

- `schemaic-core` — models, persisted UI state (`persist.rs`), transcript types, and the
  pure, unit-tested SQL/edit/export logic: `sql.rs` (one `skip_noncode` tokenizer → statement
  splitting, the unsafe-statement guard, the AI read-only gate — the *single* SQL boundary lexer;
  the UI's `tokenize_range`/`syntax_errors`/`sql_highlight` all build on it so they agree on
  string/backtick/`#`/`--`/`/* */` boundaries by construction), `edit.rs` (`analyze_edit` →
  `EditModel`, the write-back updatability analysis), `export.rs` (CSV/JSON/SQL export),
  `diff.rs` (`line_diff`/`build_diff_rows` for the Ctrl+K preview),
  `history.rs` (query-history model + `push`/`clear_conn`/`preview`/`relative_time`,
  persisted to `history.json`), `format.rs` (per-column display formatters:
  `ColumnFormat`/`apply` — epoch→datetime, bytes, bool — display-only, edit/copy
  stay raw; rules persisted to `format.json`), `schema::TableInfo::create_ddl` (the
  `CREATE TABLE`/`VIEW` skeleton), and `plan.rs`
  (`QueryPlan::from_result` — parse an `EXPLAIN` result set into a displayable table +
  heuristic warnings: full scan / filesort / temp table; `to_prompt_text` for the AI), and
  `text_ops.rs` (editor Ctrl+/ line-comment toggle `toggle_line_comment`, plus
  `find_matches`/`contains_ignore_ascii_case` for the editor + grid find bars — all pure,
  ASCII-case-insensitive, byte-offset-preserving, tested), and `sqlfmt.rs`
  (`format_sql` — the Ctrl+Alt+L SQL pretty-printer: re-flows whitespace/indent/line-breaks in
  the "block" style, **preserving keyword case verbatim**; built on the same `skip_noncode`
  boundary lexer so `#`/`--` comments, strings, and backtick identifiers pass through untouched;
  indent unit follows the editor tab-width/soft-tabs settings). The
  UI/app keep thin wrappers over these; the regression tests live here too.
- `schemaic-db` — MySQL/MariaDB connectivity (`mysql_async`) + SSH tunnels. Populates each
  result column's `origin` (real table/column + key flags) from the wire protocol. The
  connection **identity** is the `Db` handle (`Db::connect(&Connection, tunnel_port)`), not a
  `mysql://user:pass@host/db` URL: credentials go to the driver via `OptsBuilder` (so passwords
  with `@ / # ? %` need no escaping, and no plaintext URL is threaded anywhere).
  `fetch_query`/`run_batch`/`fetch_schema`/`ping`/`commit_writes`/`refetch_rows` are `Db` methods
  taking the target database per call. SSH tunnels return a `TunnelHandle` (drop → listener/port freed) with
  keepalives + TOFU host-key verification (`ssh_known_hosts.json`).
- `schemaic-ai` — persistent `claude` CLI session (stream-json), turn parsing.
- `schemaic-term` — terminal panel + shell.
- `schemaic-ui` — the Floem UI. `lib.rs` is large (being split into modules);
  it holds the central `Ui` struct (threaded everywhere) and the `workspace`/panel views. `Ui`
  is split per-domain: `Copy` signal bundles (`TabsUi`/`SchemaUi`/`ConnUi`/`AiUi`/
  `TermUi`/`LayoutUi`/`OverlayUi`) + `Rc<…Actions>` callback bundles — so `ui.run` is
  `ui.tab_actions.run`, `ui.db_nodes` is `ui.schema.db_nodes`, the tabs signal is `ui.tabs_ui.tabs`.
  Extracted so far: `consts.rs` (layout/dimension constants, glob-imported), `widgets.rs`
  (reusable widgets: the `menu_panel`/`MenuEntry` popup system, `modal_title`/`panel_style`/
  `menu_item_style`, `window_size`, the `autohide`/`shift_hscroll` scroll wrappers,
  `section_title`/`centered_msg`/`toggle_icon`, and `jump_to_bottom_button`),
  `markdown.rs` (the AI-chat `render_markdown`/`CodeActions`/`code_block` — pulldown-cmark),
  `settings.rs` (the three settings modals + shared controls),
  `connection_form.rs` (the Manage Connections modal + password-mask logic + tests),
  `diff_view.rs` (the Ctrl+K diff preview), `history_panel.rs` (the Query History
  right-column panel: per-connection newest-first list, click-to-open-in-tab, clear),
  `plan_view.rs` (the Query Plan modal:
  the `EXPLAIN`/`EXPLAIN ANALYZE` table + warnings + "Ask AI" button, driven by
  `TabsActions::run_plan` → `Db::explain`), `ai_panel.rs` (the AI Assistant panel:
  `ai_panel`/`message_bubble`/`render_segments`/`tool_chip`/`assistant_footer`),
  `overlays.rs` (the absolutely-positioned popups: connection/active-db/schema menus,
  the schema context menu, the generic grid popup menu, Find-Anywhere, the error modal),
  `schema_tree.rs` (the SCHEMA sidebar: `schema_panel` + the db/table/column/key row
  builders and keyboard nav), `completion.rs` (SQL autocomplete: `recompute_completions`/
  `accept_completion`/`completion_popup` + the context engine; `SQL_KEYWORDS` is pub(crate)),
  `tabs.rs` (the query-tab strip), `grid.rs` (the whole results grid: `GridState`/`GridCtx`,
  cells/rows/header, sort/freeze/inline-edit, export, value viewer — `results_view`/`loaded_view`
  are the entry points), `editor_pane.rs` (the SQL editor pane: `query_pane` + the Ctrl+K/cmdk
  popup, typo squiggles, statement highlight, custom scrollbars), plus
  `theme.rs`/`themes.rs`/`icons.rs`/`fonts.rs`/`sql_highlight.rs`. `lib.rs` is now ~2.6k lines
  (from 11.3k) — it holds the `Ui` struct + bundles, the shared model/state types,
  `workspace`/`body`/`center`/`header`/`footer`, the resize handles, `edit_field`/`FieldCfg`, and
  the terminal panel.
- `schemaic-app` — `main.rs` wires signals + callbacks and builds the `Ui`; also the
  built-in MCP server (`--mcp-serve`) the AI panel talks to. A query tab's identity is
  `(conn_id, database)`; the app resolves `conn_id` → a `Db` at run time (`db_for`), so a tab
  keeps running against the connection it was opened under after a connection switch.
  The MCP subprocess receives its DB endpoint as JSON in `$SCHEMAIC_MCP_ENDPOINT`, set via a
  per-session temp `--mcp-config` file (removed on session drop) — never a command-line arg, so
  credentials don't leak to other same-user processes. The pure free-function
  clusters are split out: `claude_cli.rs` (`claude` binary discovery — PATH/PATHEXT/override
  resolution) and `ai.rs` (`AiSession`/`start_ai_session` streaming, MCP-config plumbing,
  `ai_context`/`inline_system_prompt`). The reactive wiring (the `app_view` closures) stays in
  `main.rs`.

## Architecture invariants (don't regress these)

These are load-bearing invariants; re-introducing the anti-patterns they guard against is a regression:

- **One SQL boundary lexer.** Any code that scans SQL for string / `-- ` / `#` / `/* */` /
  backtick boundaries MUST build on `schemaic_core::sql::skip_noncode` (statement split, WHERE
  guard, AI read-only gate, `tokenize_range`, `syntax_errors`, `sql_highlight`). Never hand-roll a
  second scanner — five drifting copies was the original bug.
- **Connection identity is the `Db` handle / `conn_id`, never a `mysql://user:pass@host/db` URL.**
  Credentials go through `OptsBuilder`; they never appear in a URL, a command-line arg, or a log.
  The MCP subprocess gets its endpoint via a temp `--mcp-config` file, not argv. Don't add new
  plaintext-secret surfaces.
- **Own per-entity signals in a child `Scope`; dispose it *deferred*.** A `Tab` / `ConnNode` creates
  its signals in `parent.create_child()`, and the code that removes it disposes that scope via
  `exec_after(Duration::ZERO, …)` — one tick later, *after* the keyed `dyn_container` has rebuilt
  and unmounted the old view. Disposing synchronously frees signals a still-mounted view reads this
  frame → disposed-signal panic. Same deferral for any "replace + free" of scoped state.
- **Themable colors reach reactive styles as `fn() -> Color`, never a captured `Color`.** A `Color`
  read once at build freezes and won't follow a live theme switch; pass the theme fn and call it
  inside the `.style(move |s| …)` closure (see `FieldCfg::background`/`text_color`).
- **Pure logic lives in `schemaic-core` with unit tests** — SQL boundaries, edit-model analysis,
  export (incl. CSV formula-injection guard), diff, DDL. The UI keeps thin wrappers.
- **Write-back is transactional with a 1-row safety net.** `commit_writes` runs a `GridWrite`
  (row `DELETE`s → cell `UPDATE`s → new-row `INSERT`s) in one transaction and requires each
  statement to affect exactly 1 row (else roll back all) — so an over-optimistic updatability
  analysis can never corrupt data. A commit that includes inserts or deletes full-re-runs the query
  (row membership/order changed); pure UPDATE commits still splice in place.
- **Identifier scanning treats bytes `>= 0x80` as word bytes** so Unicode identifiers tokenize
  whole (`is_word_byte`, `tokenize_range`, `syntax_errors`).
- **Splitting `lib.rs` / `main.rs`:** grep the line range for interleaved unrelated `fn`s *first*;
  a helper still used by code that stays goes to `widgets.rs` (glob-imported), not the new leaf
  module; mark cross-called items `pub(crate)`; build + `cargo fmt` + smoke-launch each step.

## Build & run

- `cargo build` / `cargo run -p schemaic-app`.
- **Windows gotcha:** if the app is running, the linker can't overwrite
  `target/debug/schemaic.exe` ("Access is denied"). Stop it first
  (`Get-Process schemaic | Stop-Process -Force`) before rebuilding.

## Commits & releases

- **Never commit unless the user explicitly asks.** Making edits does **not** imply
  committing them — leave changes in the working tree for the user to review. Run
  `git commit` only on an explicit request (same for `git tag` / `git push`).
  Amending an existing commit is fine when the user is iterating on one.
- **Commit format: Conventional Commits** — `type(scope): subject`, subject in
  imperative mood, no trailing period, lower-case after the colon. Types:
  `feat` / `fix` / `refactor` / `perf` / `docs` / `test` / `chore` / `build` / `ci`.
  Scope is the crate or module the change centers on (`grid`, `editor`, `schema`,
  `ai`, `sql`, `theme`, `db`, `ci`, …); omit it only when the change is
  cross-cutting. Optional body (blank line first) explains the *why*. Every commit
  message ends with the trailer:
  `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>`.
  Example: `feat(grid): add row cloning via context menu`.
- **Version bumps are explicit-only.** Bump the version **only** when the user
  asks ("bump the version to `x.y.z`", "cut a `x.y.z` release"). **Never** bump it as
  a side effect of an unrelated commit — a normal commit leaves
  `[workspace.package].version` untouched. When asked, edit **one** place —
  `[workspace.package].version` in the root `Cargo.toml`. All crates inherit it via
  `version.workspace = true`; never set a per-crate `version`. Commit that change as
  `chore: release vX.Y.Z` (or `chore(release): …`).
- **Releases are tag-driven.** Bump the version, commit, then
  `git tag vX.Y.Z && git push origin vX.Y.Z`. Keep the tag (`vX.Y.Z`) and the
  `Cargo.toml` version in sync. Pushing the tag triggers `.github/workflows/release.yml`,
  which builds Linux + Windows binaries and attaches them to a GitHub Release;
  `.github/workflows/ci.yml` runs fmt + clippy (`-D warnings`) + `cargo deny` + build/test
  on push/PR. Keep the tree green against those before tagging.

## UI conventions

- **No pointer cursor on buttons/icons.** Native apps keep the arrow cursor on
  controls — a pointer cursor makes it feel like a web app. Use the default cursor;
  reserve `CursorStyle::Text` for text inputs. (A genuine hyperlink may keep
  `CursorStyle::Pointer`.)
- **Colors live in `theme.rs`** as named functions — add one rather than inlining a
  hex literal at the call site. The functions read the *active* theme from
  `themes.rs` (a reactive signal), so calling one inside a `.style(…)` closure makes
  that view follow a live theme switch for free.
- **Theming (`themes.rs`)**: two independent axes — `UiTheme` (chrome: dark/light)
  and `EditorTheme` (SQL editor surface + syntax tokens: One Dark Pro / Tokyo Night /
  Catppuccin Latte). A theme is a flat struct of named colour roles (Zed-ish, hex →
  easy to add / import from JSON later). The active themes live in `Scope`-owned
  global `RwSignal`s; `theme::set_ui` / `theme::set_editor` swap them. The choice is
  persisted (`ui_theme` / `editor_theme` in `UiState`) and seeded via `theme::init`
  *before* the view builds. Editor tokens re-highlight on switch because
  `SqlStyling::id()` returns `theme::editor_generation()`.
  - **Live-switch caveat**: a colour read *inside* a reactive `.style(move |s| …)`
    closure updates instantly; a colour captured *by value* (stored in a struct/var
    and applied later) freezes at build time and only refreshes on restart. Prefer
    passing a `fn() -> Color` over a `Color` for anything themable (see
    `FieldCfg::background`).
- **Reactive text**: this codebase uses `dyn_container` for reactive views (no
  `floem::views::label`).
- Small visual tweaks: build only and let the user verify. Reach for the screenshot
  harness for new features / behavior debugging, or when asked.

## Floem 0.2 gotchas (learned the hard way)

- **One `on_scroll` per scroll** — setting it twice clobbers. The `autohide` helper
  sets its own; a scroll that needs custom `on_scroll` must inline `autohide_state()`
  (see the results grid + AI convo).
- **No `opacity` property.** Fade via color alpha (`multiply_alpha`) + `.transition_*`.
  Toggle visibility with `.hide()` / `.flex()` (display none/flex).
- **Inherited color doesn't animate to a child.** A `.transition_color()` on a parent
  won't smoothly fade a child svg's `currentColor` — set the color + transition on the
  element itself.
- **Style precedence**: a view's own (direct) style beats ancestor class styles; among
  ancestor class rules the nearest ancestor wins. Nest class overrides accordingly
  (e.g. dropdown popup restyle nests under `ListClass`).
- **`DoubleClick` consumes the second `PointerUp`** — clear drag/press state in the
  double-click handler too, not only in `PointerUp`.
- **Absolute overlays** (placeholder text, action bars) intercept clicks — add
  `.pointer_events(|| false)` so clicks fall through to what's beneath.
- **Deferred layout**: `exec_after(Duration::ZERO, …)` runs after layout settles — used
  so `scroll_to(bottom)` clamps against the new content height, not the stale one.
- **`RwSignal::set` never dedups** — setting a signal to the value it already holds
  still notifies, re-running dependent `dyn_container`s (which dispose + rebuild their
  child scope and its owned signals). Guard panel reveals like
  `if !matches!(right_panel.get_untracked(), Ai) { set(Ai) }` — a redundant `set(Ai)`
  while the AI panel is open disposes its `elapsed_ms` mid-update, and the rebuilt
  pending footer then panics reading the freed signal (`Option::unwrap()` on `None`).
- **Don't read a locally-scoped signal inside a `dyn_container` child keyed on a
  *parent/shared* signal.** A `dyn_container(move || shared.get(), move |_| … reads
  local_sig …)` rebuilds its child when `shared` changes — and if `shared` changes
  *while the enclosing view is being disposed* (e.g. `active_db` updates as the query
  pane is replaced on opening a table), the rebuild reads the already-freed
  `local_sig` → `Option::unwrap()` on `None`. Fix: read the local signal in a stable
  outer scope and let the child inherit (e.g. put the hover `color()` on the parent
  `h_stack`, not inside the `active_db`-keyed child — the db-selector crash). Reading
  *global* signals (theme, app-level `connections`/`active_conn`) inside such a child
  is fine — they never dispose.
- **No `text-align` in Floem 0.2.** `text_input` paints text at a fixed left origin
  and clips-to-cursor on overflow; `Style` only has `text_overflow` (Wrap/Ellipsis/
  Clip). To right-align an inline editor (numeric grid cells), pad the left by exactly
  `col_w − measured_text_w` — measure with a throwaway `TextLayout` at `FONT_BODY`
  (same global `FontSystem` → pixel-exact), recomputed reactively on the buffer. See
  `grid::measure_text_px` / the numeric `data_cell` editor.
- **SQL editor padding is a no-op; inset via a wrapper.** The editor is a scroll view —
  its own `padding_*` is ignored (content scrolls under it). Give the code breathing
  room by wrapping the editor in a container that carries the border + padding (see
  `editor_box`); the editor fills it flush. Top padding shifts the editor's content
  origin, so the `points_of_offset`-anchored overlays (completion popup, statement
  highlight box, squiggles, Ctrl+K, run menu) each add back `EDITOR_PAD_TOP` to their
  `y`. Consequence: the built-in editor scrollbars float at the editor's *content* edge
  and can only be inset *inward* (`track_inset`), never toward the border — so with a
  padded wrapper the bars would sit at the top of the gap, over the last line/column.
  **Solved with custom overlay scrollbars** (`v_scrollbar`/`h_scrollbar` in the
  `editor_area` stack): the built-in bars are hidden (`.class(Handle, …)` set to
  zero-`Thickness` + transparent), and two `empty()` thumbs are drawn pinned to the
  *border* (`inset_right/bottom(3)`) with `autohide_state()` for the 3s auto-hide.
  Geometry derives from the editor's live `ed.viewport` signal (scroll offset `x0`/`y0`
  + visible `width()`/`height()`) vs. content size (`ed.max_line_width()` and
  `(ed.last_line()+1) * ed.line_height(0)`; `ScrollBeyondLastLine` defaults `false`, so
  no bottom margin to account for), factored into `v_geo`/`h_geo` helpers shared by the
  style closure and the drag handler. The thumb `.style()` closures read `viewport.get()`
  (re-runs on scroll/resize) **and** `query.get()` (content size isn't a signal, so this
  is the edit trigger); a `create_effect` tracking `viewport` pokes the hide timer.
  **Draggable**: on `PointerDown` the thumb records the grab offset + `id.request_active()`
  (pointer capture, so moves keep coming when the cursor leaves the moving thumb) and, on
  each `PointerMove`, converts the pointer delta to an absolute scroll via
  `ed.scroll_to.set(Some(Vec2))` (`scroll_to` is `Option<Vec2>`, **not** `Point`). Same
  moving-view drag trick as `v_resize_handle`. Hover/drag use `scrollbar_hover()`; the
  thumbs carry `CursorStyle::Default` (they sit over the editor's `Text` cursor).
- **Shift+wheel → horizontal scroll in the editor.** The editor owns its scroll
  internally, so the `shift_hscroll` helper (which wraps our own `scroll()`) can't reach
  it. Instead register a `PointerWheel` listener directly on the internal scroll view —
  reached via `ed.editor_view_id.get_untracked().and_then(|c| c.parent())` (the content
  view's parent *is* the scroll) + `ViewId::add_event_listener`. Floem's `Scroll`
  runs its own registered listeners in `event_after_children` *before* its default wheel
  scrolling, so returning `EventPropagation::Stop` for a shift+wheel suppresses the
  vertical scroll; the horizontal delta is pushed through `ed.scroll_delta` (Windows
  delivers shift+wheel as a vertical `delta.y`, so map it to x). Note the event flow:
  for nested views a child's `event_after_children` (its scrolling) runs *between* the
  parent's `before_children` and `after_children`, and a child that processes a pointer
  event stops propagation — so a `PointerWheel` `on_event` on an *ancestor* never sees a
  wheel the inner scroll consumed. You must target the scroll's own `ViewId`.
- **A programmatic `cursor` change shows a phantom caret on an *unfocused* editor.**
  `edit_field`'s signal→doc reconcile (loading a connection, New, clear-×) sets the
  editor's `cursor` to preserve the caret position. But floem runs an internal effect
  (`editor/mod.rs`, "reset cursor blinking whenever the cursor changes") that tracks
  `ed.cursor` and calls `cursor_info.reset()` → `hidden = false` + starts blinking —
  so *any* cursor write makes the caret appear, even with no keyboard focus. The field
  then looks focused (our own `focused` signal stays false; it's the blinking caret, not
  the active border). This is why filling/clearing a field made Name/User/Password look
  focused while unchanged fields (Host/Port) didn't. Fix: only write `cursor` in the
  reconcile when `focused.get_untracked()`; leave it alone otherwise. The stale offset is
  safe — floem's offset→line math clamps (`offset.min(text.len())`), and a real click
  sets a fresh caret. (Don't try to hide the caret *after* the cursor write — floem's
  reset effect runs after your effect and re-shows it.)
- **A perpetual `exec_after` self-rescheduling tick must read its signals with
  `try_get_untracked`.** The terminal cursor-blink tick reschedules itself forever; at
  app shutdown the reactive scope disposes its signals, and the last in-flight timer then
  panics on `get_untracked` (`Option::unwrap` on `None`). Guard every signal read in such
  a tick with `try_get_untracked` and bail (stop rescheduling) once any returns `None`.

## Popup menus (`menu_panel`)

Context menus / dropdowns are custom themed overlays, not Floem's native `Menu`
(native menus render OS-styled and clash with the dark theme). The reusable one is
`menu_panel(entries: Vec<MenuEntry>, close)` — build a `Vec<MenuEntry>` (`Action` /
`Sub` / `Separator`) and it renders the themed panel; the caller positions it
absolutely (e.g. at `last_mouse`). Used by the schema right-click menu
(`context_menu_overlay`); intended for the results grid too.

- **Nested submenus**: a `Sub` entry hover-expands a child `menu_stack` anchored to
  the parent row's right edge via `inset_left_pct(100.0)` (+ `inset_top(-6.0)` so the
  submenu's first item lines up with the parent). Recursive — each level owns its own
  `open_sub` signal, so depth is unbounded (two levels is the norm).
- **Hover intent (no timers)**: entering a leaf row clears `open_sub`; entering a
  submenu row sets it; nothing closes on *leave*. Because the submenu is flush with
  the panel's right edge, moving diagonally onto it never crosses a gap, so it stays
  open — the classic submenu-closes-on-diagonal problem is avoided structurally.
- **Dismissal**: the panel `on_event_stop`s its own pointer-downs, so the root-level
  "pointer-down anywhere closes the menu" handler (in `workspace`) only fires for
  clicks *outside*. Escape and any action also call `close`. Submenus are descendants
  in the view tree (absolute positioning doesn't change parentage), so their clicks
  are absorbed by the root panel too.
- **Edge-flipping**: submenus flip left (`inset_right_pct(100)`) if they'd spill past
  the window's right edge, and shift up if past the bottom — decided from the parent
  row's window position (`on_move`/`on_resize`) + the live `window_size()` global (a
  detached-`Scope` signal set from `workspace`'s root `on_resize`). The generic
  `popup_menu_overlay` flips the *whole* panel the same way at the cursor. Size checks
  use conservative estimates (menu width ≈ 210, row ≈ 34px), not measured sizes, so
  there's no open-then-flip flicker.
- **Two menu channels**: the schema tree uses `ui.context_menu` (typed `CtxMenu`) +
  `context_menu_overlay`; everything else uses the generic `ui.popup_menu`
  (`RwSignal<Option<Vec<MenuEntry>>>`) + `popup_menu_overlay`. The grid sets
  `ui.popup_menu` from its header/cell right-click handlers (see the data-grid section).
  Both overlays live at the workspace root (so they position in window coords) and are
  closed by the root pointer-down handler.

## Data grid (results grid)

The results grid (`grid_view` in `lib.rs`, ~`fn grid_view`) is built around a `GridState`
struct — a bundle of `RwSignal`s (so it's `Copy`) created once per result set and threaded
into every cell/handler closure. It holds column widths, the selection (`active`/`anchor` in
**display** coords, not data-row coords, so selection stays put visually on sort), the
display→data-row `order` (for copy/export/viewer/edit), the value-viewer/freeze/edit
toggles, the `dirty` edit map, and `vp`/`scroll_to`/`focus_id` for keyboard nav.

- **Layout is two panes** side by side (`h_stack`): a **frozen pane** (row-number gutter +
  an optional frozen column) and a horizontally-scrolling **data pane**. Rebuilt by a
  `dyn_container` keyed on `(sort, frozen)`. **Freeze is per-column, any column**:
  `gs.frozen` is an `RwSignal<Option<usize>>` holding the frozen column's *absolute* index,
  set from the header right-click menu ("Freeze" / "Unfreeze", before "Copy") — there's no
  toolbar freeze button. The data pane renders `data_cols` = `(0..ncols)` minus the frozen
  index, in order (passed as an `Arc<Vec<usize>>` to `data_row`); cells always carry their
  *absolute* `ci` so selection/resize/sort indices stay consistent across a non-contiguous
  column set. The frozen pane's width = `GUTTER_W + widths[frozen]`.
- **⚠️ Scroll-sync rule (cost me a hang):** a scroll view must **never both read and write
  the same offset signal** — doing so re-enters its own layout and hangs the UI thread
  (window ops then block; `IsHungAppWindow` = true). Use strict one-writer / one-reader, like
  the header↔body `h_off` sync. Here: the **data pane writes `vscroll`** (its `on_scroll`)
  and reads `gs.scroll_to` (the keyboard command channel); the **frozen pane reads `vscroll`**
  (its `scroll_to`) and has **no `on_scroll`** — it's a pure follower, and its own wheel is
  blocked (`on_event(PointerWheel, |_| Stop)`) so it can't scroll independently and desync.
- **Column widths** are per-column (`gs.widths`), estimated from content on load; the header's
  right-edge `col_resize_handle` drags to resize (same moving-view trick as `v_resize_handle`)
  and double-clicks to auto-fit. Cells read `gs.widths` in their `.style()` so resize is live.
  Every cell/header uses `flex_shrink(0)` so the row overflows (enabling h-scroll) instead of
  squeezing.
- **Selection**: click sets `active`+`anchor`; `PointerEnter` while `selecting` extends the
  range (drag-select, no capture needed); gutter click selects the whole row. Copy (Ctrl+C or
  the toolbar) emits TSV; a lone cell copies its raw value.
- **Right-click menus** (via the generic `menu_panel` / `ui.popup_menu`): a header offers
  `Copy › CSV / JSON` of **that column's** values (a newline list / JSON array —
  `export_column_csv`/`_json`, for building arrays out of one column); a data cell offers
  `View` (value viewer), `Edit` (shortcut for `start_edit`; only on editable cells), `Copy`
  (that cell), `Set to NULL` (only on editable **and** nullable columns — stages `dirty`
  `None`), and `AI Summary` (sparkles, `key_foreign` — reveals the AI panel and asks Claude to
  summarize the value, with the source table + column name in the prompt for pattern context).
  The grid's app context
  (`source`, `db_nodes`, the `popup` signal, a `summarize` reveal+send callback, and a
  `dismiss` close-menus callback) is bundled in `GridCtx`, threaded `results_section →
  results_view/multi → loaded_view → grid_view`, then stashed in `GridState`. Because
  `GridState` is `Copy`, the `Rc` callbacks live in `RwSignal<Option<…>>` fields.
- **Menu dismissal**: grid cells consume the pointer-down (for drag-select), so the
  root-level "pointer-down closes the menu" handler never fires for clicks inside the grid.
  So the cell/header/gutter click handlers call `gs.dismiss` (closes both `ui.popup_menu`
  and `ui.context_menu`, guarded) — otherwise an open menu (grid's *or* the schema tree's)
  wouldn't close when you click in the table.
- The value viewer has no toolbar toggle — it's opened from the cell `View` menu item (or
  double-click / Enter), closed by its ✕ or Esc (grid-focused). It's a read-only, word-
  wrapped, auto-growing `edit_field` (`multiline` + `read_only`) that follows the active
  cell live via an effect → `text_sig`. It grows with content up to a cap and scrolls
  beyond; the cap is a reactive `FieldCfg.max_rows` signal set from the results-panel height
  (`(panel − 172) / 19` rows), so the viewer can expand toward the full panel while the grid
  keeps a `min_height(120)`. `edit_field` gained: `max_rows` (reactive cap) and a
  `viewport`-tracking effect that recomputes the wrapped line count on layout — needed
  because `.update` only fires on edits, so programmatically-set multiline text (the viewer)
  would otherwise be measured at zero width → one line. Callers that want wrapping must give
  the field a bounded width (`.style(|s| s.width_full())`) — its box is content-sized.
- **Inline edit writes back to the DB (DataGrip-style).** Built on per-column *provenance*:
  `schemaic-db` runs on **`mysql_async`** (not sqlx — sqlx's MySQL driver discards the
  column-definition packet's `org_table`/`org_name`/key flags), so each `Column` (see
  `schemaic-core/model.rs`) carries `origin: Option<ColumnOrigin>` — the real
  `database`/`table`/`column` + `ColumnFlags` (pk/unique/not_null/auto_increment), or `None`
  for an expression/aggregate (read-only). `analyze_edit` builds an `EditModel` per result:
  which columns are editable + each base table's WHERE key (schema PK first — authoritative for
  composite keys; else a fully-present unique NOT NULL index; else the wire PK flags when the
  schema isn't loaded). Flow: **double-click** an editable cell (or Enter) → inline editor;
  **Enter** stages the value into `gs.dirty` and paints the cell `theme::grid_edit_staged()`
  (#509950); **Ctrl+Enter** or the toolbar ✓ (shown when there are staged changes) calls
  `commit_grid`, which bundles a `GridWrite { updates, inserts }` (one `RowEdit` per (table,row)
  from `dirty`, one `RowInsert` per pending row) and hands it to the app's `commit_edits` callback.
  `schemaic_db::commit_writes` runs them in **one transaction**, requiring each statement to
  affect **exactly 1 row** (else roll back all + report) — the safety net that makes an
  over-optimistic analysis incapable of corrupting data. On success the app re-runs the tab's
  query (grid shows DB truth); on failure the error shows in the toolbar and the green edits
  stay. There is no global "Edit" toggle. A read-only cell's double-click opens the value viewer.
- **Row actions: new / clone / delete.** All gated on a single writable table
  (`EditModel::insert_target()`; hidden for joins / read-only connections) and committed in the
  shared `GridWrite` transaction (`commit_writes` runs **deletes → updates → inserts**, each
  required to affect exactly 1 row).
    - **New row (INSERT):** the toolbar **"+ Row"** button appends a blank pending row
      (`gs.new_rows`, a `Vec<HashMap<col, Option<String>>>`), rendered below the real rows at
      display index `nrows + pending_index` with a `*` gutter marker + faint green wash, and opens
      its first editable cell. Cells stage via `stage_new` (unset = server default; `Some("")`
      clears back to default). Unset cells preview what they'll do: `<auto>` / `<required>` /
      `<null>` / `<default>` (from the wire `auto_increment` / `no_default` / `not_null` flags).
      Tab / Enter hop to the next editable cell (`advance_edit`).
    - **Clone (Duplicate row):** right-click **Duplicate row** seeds a new pending row from an
      existing row's values via `add_cloned_row` (every editable column except auto-increment).
    - **Delete:** right-click **Delete row** (toggle) or the **Del** key marks a real row
      (`gs.del_rows`, a `HashSet<data_row>`) with a red wash; marking clears any staged edits on
      it (a delete supersedes an update). `build_deletes` keys each `RowDelete` by the table's
      `key_cols` + original values.
  An insert or delete changes row membership/order, so those commits **full-re-run** the query
  (only pure-UPDATE commits splice in place). A NOT-NULL-no-default omission (or a duplicate-key
  clone) fails the transaction and surfaces the error — nothing half-applied.
- **Type-aware headers** show the SQL `type_name` under the name (two-line header, height
  `GRID_HEADER_H`). A sorted column's name + chevron use `theme::grid_sort()`; a column whose
  cells are selected gets a `theme::grid_col_sel()` header background. **Key icons** (PK =
  gold key-round, single-column index = blue key-square, FK = purple key-square, colours
  shared with the schema tree via `key_primary/index/foreign`) come from `column_key_map`,
  which cross-references the tab's `source` table against the loaded schema (`db_nodes`). It's
  threaded `results_section → results_view/results_multi → loaded_view → grid_view` as the
  `source`/`db_nodes` signals. Only populated when the tab was opened from a table and its
  schema is loaded (both true when you open a table from the tree); arbitrary SELECTs get no
  key icons. Nullable markers still deferred.
- **Deferred: column virtualization.** Rows are virtualized (`virtual_stack`); columns are not
  (every row builds all its cells). Fine for typical results; heavy for very wide tables. Doing
  it means, per row, rendering only columns intersecting `[h_off, h_off+viewport_w]` plus left/
  right spacer widths — which fights the current `h_stack`/resize/freeze layout, so it was left
  as a follow-up rather than risk the working grid.
